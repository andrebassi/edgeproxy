package main

import (
	"crypto/rand"
	"database/sql"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"os"
	"strconv"
	"strings"
	"sync/atomic"
	"time"

	_ "github.com/lib/pq"
)

var regionNames = map[string]struct {
	EN string
	PT string
}{
	"gru": {"Sao Paulo, Brazil", "Sao Paulo, Brasil"},
	"gig": {"Rio de Janeiro, Brazil", "Rio de Janeiro, Brasil"},
	"iad": {"Virginia, USA", "Virginia, EUA"},
	"ord": {"Chicago, USA", "Chicago, EUA"},
	"lax": {"Los Angeles, USA", "Los Angeles, EUA"},
	"scl": {"Santiago, Chile", "Santiago, Chile"},
	"fra": {"Frankfurt, Germany", "Frankfurt, Alemanha"},
	"lhr": {"London, UK", "Londres, Reino Unido"},
	"cdg": {"Paris, France", "Paris, Franca"},
	"nrt": {"Tokyo, Japan", "Toquio, Japao"},
	"sin": {"Singapore", "Cingapura"},
	"syd": {"Sydney, Australia", "Sydney, Australia"},
}

var (
	region         string
	hostname       string
	requestCount   uint64
	bytesServed    uint64
	startTime      time.Time
	db             *sql.DB
)

func main() {
	region = os.Getenv("FLY_REGION")
	if region == "" {
		region = "local"
	}

	// Get hostname - use FLY_ALLOC_ID or machine hostname
	hostname = os.Getenv("FLY_ALLOC_ID")
	if hostname == "" {
		hostname, _ = os.Hostname()
	}
	// Truncate to first 8 chars for display
	if len(hostname) > 8 {
		hostname = hostname[:8]
	}

	port := os.Getenv("PORT")
	if port == "" {
		port = "8080"
	}

	startTime = time.Now()

	// Initialize database if configured
	initDB()

	// Basic endpoints
	http.HandleFunc("/", handleRequest)
	http.HandleFunc("/health", handleHealth)

	// Performance test endpoints (v2)
	http.HandleFunc("/benchmark", handleBenchmarkPage)
	http.HandleFunc("/api/download", handleDownload)
	http.HandleFunc("/api/upload", handleUpload)
	http.HandleFunc("/api/latency", handleLatency)
	http.HandleFunc("/api/stats", handleStats)
	http.HandleFunc("/api/info", handleInfo)

	// RDS Benchmark endpoints (v4)
	http.HandleFunc("/api/rds/benchmark", handleRDSBenchmark)
	http.HandleFunc("/api/rds/health", handleRDSHealth)

	fmt.Printf("Backend v2 running in region [%s] on port %s\n", region, port)
	fmt.Printf("Benchmark page: http://localhost:%s/benchmark\n", port)

	if err := http.ListenAndServe(":"+port, nil); err != nil {
		fmt.Fprintf(os.Stderr, "Failed to start server: %v\n", err)
		os.Exit(1)
	}
}

func handleHealth(w http.ResponseWriter, r *http.Request) {
	w.WriteHeader(http.StatusOK)
	fmt.Fprintf(w, "OK - Region: %s", region)
}

func getEnv(key, defaultValue string) string {
	if value := os.Getenv(key); value != "" {
		return value
	}
	return defaultValue
}

func initDB() {
	dbHost := getEnv("DB_HOST", "")
	if dbHost == "" {
		fmt.Println("DB_HOST not set, RDS benchmark disabled")
		return
	}

	dbPort := getEnv("DB_PORT", "5432")
	dbUser := getEnv("DB_USER", "postgres")
	dbPassword := getEnv("DB_PASSWORD", "")
	dbName := getEnv("DB_NAME", "contacts")

	connStr := fmt.Sprintf("host=%s port=%s user=%s password=%s dbname=%s sslmode=disable",
		dbHost, dbPort, dbUser, dbPassword, dbName)

	var err error
	db, err = sql.Open("postgres", connStr)
	if err != nil {
		fmt.Printf("Failed to open database: %v\n", err)
		return
	}

	db.SetMaxOpenConns(10)
	db.SetMaxIdleConns(5)
	db.SetConnMaxLifetime(time.Minute * 5)

	if err := db.Ping(); err != nil {
		fmt.Printf("Failed to ping database: %v\n", err)
		db = nil
		return
	}

	fmt.Printf("Database connected: %s\n", dbHost)
}

func handleRDSHealth(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Content-Type", "application/json")
	w.Header().Set("X-Fly-Region", region)

	dbHost := getEnv("DB_HOST", "not configured")

	result := map[string]interface{}{
		"region":  region,
		"db_host": dbHost,
	}

	if db == nil {
		result["status"] = "disabled"
		result["message"] = "Database not configured"
	} else if err := db.Ping(); err != nil {
		result["status"] = "error"
		result["message"] = err.Error()
	} else {
		result["status"] = "connected"
	}

	json.NewEncoder(w).Encode(result)
}

func handleRDSBenchmark(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Content-Type", "application/json")
	w.Header().Set("X-Fly-Region", region)

	dbHost := getEnv("DB_HOST", "not configured")

	if db == nil {
		json.NewEncoder(w).Encode(map[string]interface{}{
			"error":   "Database not configured",
			"region":  region,
			"db_host": dbHost,
		})
		return
	}

	iterations := 10
	if iter := r.URL.Query().Get("iterations"); iter != "" {
		if n, err := strconv.Atoi(iter); err == nil && n > 0 && n <= 100 {
			iterations = n
		}
	}

	readLatencies := make([]float64, iterations)
	insertLatencies := make([]float64, iterations)

	// Run READ benchmarks
	for i := 0; i < iterations; i++ {
		start := time.Now()
		var count int
		db.QueryRow("SELECT COUNT(*) FROM contacts").Scan(&count)
		readLatencies[i] = float64(time.Since(start).Microseconds()) / 1000.0
	}

	// Run INSERT benchmarks
	for i := 0; i < iterations; i++ {
		start := time.Now()
		name := fmt.Sprintf("Bench-%s-%d-%d", region, time.Now().UnixNano(), i)
		email := fmt.Sprintf("bench-%d@test.local", time.Now().UnixNano())
		db.Exec(`INSERT INTO contacts (name, email, notes) VALUES ($1, $2, $3)`,
			name, email, "Benchmark")
		insertLatencies[i] = float64(time.Since(start).Microseconds()) / 1000.0
	}

	// Calculate stats
	calcStats := func(latencies []float64) (avg, min, max float64) {
		if len(latencies) == 0 {
			return 0, 0, 0
		}
		min = latencies[0]
		max = latencies[0]
		var sum float64
		for _, l := range latencies {
			sum += l
			if l < min {
				min = l
			}
			if l > max {
				max = l
			}
		}
		avg = sum / float64(len(latencies))
		return
	}

	readAvg, readMin, readMax := calcStats(readLatencies)
	insertAvg, insertMin, insertMax := calcStats(insertLatencies)

	result := map[string]interface{}{
		"region":           region,
		"db_host":          dbHost,
		"iterations":       iterations,
		"read_avg_ms":      readAvg,
		"read_min_ms":      readMin,
		"read_max_ms":      readMax,
		"insert_avg_ms":    insertAvg,
		"insert_min_ms":    insertMin,
		"insert_max_ms":    insertMax,
		"read_latencies":   readLatencies,
		"insert_latencies": insertLatencies,
		"timestamp":        time.Now().UTC().Format(time.RFC3339),
	}

	json.NewEncoder(w).Encode(result)
}

func handleRequest(w http.ResponseWriter, r *http.Request) {
	atomic.AddUint64(&requestCount, 1)

	lang := "en"
	if r.URL.Query().Get("lang") == "pt" {
		lang = "pt"
	}
	acceptLang := r.Header.Get("Accept-Language")
	if strings.HasPrefix(strings.ToLower(acceptLang), "pt") {
		lang = "pt"
	}

	w.Header().Set("Content-Type", "text/plain; charset=utf-8")
	w.Header().Set("X-Fly-Region", region)
	w.Header().Set("X-Request-Count", strconv.FormatUint(atomic.LoadUint64(&requestCount), 10))

	response := buildResponse(region, lang)
	fmt.Fprint(w, response)
}

// handleInfo returns JSON with backend info
func handleInfo(w http.ResponseWriter, r *http.Request) {
	atomic.AddUint64(&requestCount, 1)

	names, ok := regionNames[region]
	if !ok {
		names = struct{ EN, PT string }{region, region}
	}

	info := map[string]interface{}{
		"region":       region,
		"region_name":  names.EN,
		"hostname":     hostname,
		"uptime_secs":  int(time.Since(startTime).Seconds()),
		"requests":     atomic.LoadUint64(&requestCount),
		"bytes_served": atomic.LoadUint64(&bytesServed),
		"timestamp":    time.Now().UTC().Format(time.RFC3339),
	}

	w.Header().Set("Content-Type", "application/json")
	w.Header().Set("X-Fly-Region", region)
	json.NewEncoder(w).Encode(info)
}

// handleLatency returns minimal response for latency testing
func handleLatency(w http.ResponseWriter, r *http.Request) {
	atomic.AddUint64(&requestCount, 1)

	w.Header().Set("Content-Type", "application/json")
	w.Header().Set("X-Fly-Region", region)
	w.Header().Set("X-Server-Time", strconv.FormatInt(time.Now().UnixNano(), 10))

	json.NewEncoder(w).Encode(map[string]interface{}{
		"region": region,
		"ts":     time.Now().UnixNano(),
	})
}

// handleDownload generates random data for download speed testing
func handleDownload(w http.ResponseWriter, r *http.Request) {
	atomic.AddUint64(&requestCount, 1)

	// Default 1MB, max 100MB
	sizeStr := r.URL.Query().Get("size")
	size := 1024 * 1024 // 1MB default

	if sizeStr != "" {
		if s, err := strconv.Atoi(sizeStr); err == nil {
			size = s
			if size > 100*1024*1024 {
				size = 100 * 1024 * 1024 // max 100MB
			}
		}
	}

	w.Header().Set("Content-Type", "application/octet-stream")
	w.Header().Set("Content-Length", strconv.Itoa(size))
	w.Header().Set("X-Fly-Region", region)
	w.Header().Set("X-File-Size", strconv.Itoa(size))
	w.Header().Set("Content-Disposition", fmt.Sprintf("attachment; filename=\"test-%s-%d.bin\"", region, size))

	// Stream random data in chunks
	chunkSize := 64 * 1024 // 64KB chunks
	chunk := make([]byte, chunkSize)
	remaining := size

	for remaining > 0 {
		toWrite := chunkSize
		if remaining < chunkSize {
			toWrite = remaining
		}

		rand.Read(chunk[:toWrite])
		n, err := w.Write(chunk[:toWrite])
		if err != nil {
			return
		}
		remaining -= n
		atomic.AddUint64(&bytesServed, uint64(n))
	}
}

// handleUpload receives data for upload speed testing
func handleUpload(w http.ResponseWriter, r *http.Request) {
	atomic.AddUint64(&requestCount, 1)

	if r.Method != "POST" {
		http.Error(w, "POST required", http.StatusMethodNotAllowed)
		return
	}

	start := time.Now()
	n, _ := io.Copy(io.Discard, r.Body)
	elapsed := time.Since(start)

	speedMbps := float64(n*8) / elapsed.Seconds() / 1024 / 1024

	w.Header().Set("Content-Type", "application/json")
	w.Header().Set("X-Fly-Region", region)

	json.NewEncoder(w).Encode(map[string]interface{}{
		"region":     region,
		"bytes":      n,
		"elapsed_ms": elapsed.Milliseconds(),
		"speed_mbps": speedMbps,
	})
}

// handleStats returns server statistics
func handleStats(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Content-Type", "application/json")
	w.Header().Set("X-Fly-Region", region)

	uptime := time.Since(startTime)
	reqs := atomic.LoadUint64(&requestCount)
	bytes := atomic.LoadUint64(&bytesServed)

	json.NewEncoder(w).Encode(map[string]interface{}{
		"region":        region,
		"uptime":        uptime.String(),
		"uptime_secs":   int(uptime.Seconds()),
		"requests":      reqs,
		"bytes_served":  bytes,
		"mb_served":     float64(bytes) / 1024 / 1024,
		"reqs_per_sec":  float64(reqs) / uptime.Seconds(),
	})
}

// handleBenchmarkPage serves the HTML benchmark page
func handleBenchmarkPage(w http.ResponseWriter, r *http.Request) {
	atomic.AddUint64(&requestCount, 1)

	w.Header().Set("Content-Type", "text/html; charset=utf-8")
	w.Header().Set("X-Fly-Region", region)

	fmt.Fprint(w, benchmarkHTML)
}

func buildResponse(region, lang string) string {
	names, ok := regionNames[region]
	if !ok {
		names = struct{ EN, PT string }{region, region}
	}

	timestamp := time.Now().UTC().Format("2006-01-02 15:04:05 UTC")
	reqs := atomic.LoadUint64(&requestCount)

	var title, location, powered, footer string
	if lang == "pt" {
		title = "Voce esta conectado a"
		location = names.PT
		powered = "Powered by edgeProxy v0.1.0"
		footer = "Rede Global de Borda"
	} else {
		title = "You are connected to"
		location = names.EN
		powered = "Powered by edgeProxy v0.1.0"
		footer = "Global Edge Network"
	}

	box := `
███████╗██████╗  ██████╗ ███████╗
██╔════╝██╔══██╗██╔════╝ ██╔════╝
█████╗  ██║  ██║██║  ███╗█████╗
██╔══╝  ██║  ██║██║   ██║██╔══╝
███████╗██████╔╝╚██████╔╝███████╗
╚══════╝╚═════╝  ╚═════╝ ╚══════╝

%s

  %s
  Region: %s
  Host: %s
  Requests: %d

%s
%s

%s

Try: /benchmark for speed test
`

	return fmt.Sprintf(box, title, location, strings.ToUpper(region), hostname, reqs, powered, footer, timestamp)
}

const benchmarkHTML = `<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>edgeProxy Benchmark v2</title>
    <style>
        * { box-sizing: border-box; margin: 0; padding: 0; }
        body {
            font-family: 'SF Mono', 'Monaco', 'Inconsolata', 'Fira Code', monospace;
            background: linear-gradient(135deg, #1a1a2e 0%, #16213e 50%, #0f3460 100%);
            color: #e8e8e8;
            min-height: 100vh;
            padding: 20px;
        }
        .container { max-width: 1200px; margin: 0 auto; }

        header {
            text-align: center;
            padding: 30px 0;
            border-bottom: 1px solid #333;
            margin-bottom: 30px;
        }
        .logo {
            font-size: 2.5em;
            font-weight: bold;
            background: linear-gradient(90deg, #00d4ff, #00ff88);
            -webkit-background-clip: text;
            -webkit-text-fill-color: transparent;
        }
        .region-badge {
            display: inline-block;
            background: #00ff88;
            color: #1a1a2e;
            padding: 8px 20px;
            border-radius: 20px;
            font-weight: bold;
            margin: 15px 0;
            font-size: 1.2em;
        }

        .grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(350px, 1fr)); gap: 20px; }

        .card {
            background: rgba(255,255,255,0.05);
            border: 1px solid rgba(255,255,255,0.1);
            border-radius: 12px;
            padding: 25px;
            backdrop-filter: blur(10px);
        }
        .card h2 {
            color: #00d4ff;
            margin-bottom: 20px;
            font-size: 1.1em;
            display: flex;
            align-items: center;
            gap: 10px;
        }

        .metric {
            display: flex;
            justify-content: space-between;
            padding: 12px 0;
            border-bottom: 1px solid rgba(255,255,255,0.1);
        }
        .metric:last-child { border-bottom: none; }
        .metric-label { color: #888; }
        .metric-value { font-weight: bold; color: #00ff88; }
        .metric-value.warning { color: #ffaa00; }
        .metric-value.error { color: #ff4444; }

        button {
            background: linear-gradient(90deg, #00d4ff, #00ff88);
            color: #1a1a2e;
            border: none;
            padding: 12px 25px;
            border-radius: 8px;
            font-weight: bold;
            cursor: pointer;
            font-family: inherit;
            font-size: 1em;
            transition: transform 0.2s, opacity 0.2s;
            width: 100%;
            margin: 5px 0;
        }
        button:hover { transform: scale(1.02); }
        button:disabled { opacity: 0.5; cursor: not-allowed; transform: none; }
        button.danger { background: linear-gradient(90deg, #ff4444, #ff8800); }
        button.secondary { background: rgba(255,255,255,0.2); color: #fff; }

        .progress-bar {
            background: rgba(255,255,255,0.1);
            border-radius: 10px;
            height: 20px;
            overflow: hidden;
            margin: 10px 0;
        }
        .progress-fill {
            height: 100%;
            background: linear-gradient(90deg, #00d4ff, #00ff88);
            transition: width 0.3s;
            display: flex;
            align-items: center;
            justify-content: center;
            font-size: 0.8em;
            color: #1a1a2e;
            font-weight: bold;
        }

        .results-table {
            width: 100%;
            margin-top: 15px;
            font-size: 0.9em;
        }
        .results-table th, .results-table td {
            padding: 8px;
            text-align: left;
            border-bottom: 1px solid rgba(255,255,255,0.1);
        }
        .results-table th { color: #00d4ff; }

        .log {
            background: #0a0a15;
            border-radius: 8px;
            padding: 15px;
            font-size: 0.85em;
            max-height: 200px;
            overflow-y: auto;
            margin-top: 15px;
        }
        .log-entry { padding: 3px 0; border-bottom: 1px solid #222; }
        .log-entry.success { color: #00ff88; }
        .log-entry.error { color: #ff4444; }
        .log-entry.info { color: #00d4ff; }

        .speed-display {
            text-align: center;
            padding: 20px;
        }
        .speed-value {
            font-size: 3em;
            font-weight: bold;
            background: linear-gradient(90deg, #00d4ff, #00ff88);
            -webkit-background-clip: text;
            -webkit-text-fill-color: transparent;
        }
        .speed-unit { color: #888; font-size: 1.2em; }

        .latency-chart {
            display: flex;
            align-items: flex-end;
            height: 100px;
            gap: 3px;
            margin: 15px 0;
            padding: 10px;
            background: rgba(0,0,0,0.3);
            border-radius: 8px;
        }
        .latency-bar {
            flex: 1;
            background: #00d4ff;
            min-width: 4px;
            transition: height 0.2s;
            border-radius: 2px 2px 0 0;
        }
        .latency-bar.high { background: #ffaa00; }
        .latency-bar.very-high { background: #ff4444; }

        footer {
            text-align: center;
            padding: 30px;
            color: #666;
            margin-top: 30px;
        }
    </style>
</head>
<body>
    <div class="container">
        <header>
            <div class="logo">edgeProxy Benchmark v2</div>
            <div class="region-badge" id="regionBadge">Loading...</div>
            <p style="color: #888; margin-top: 10px">Performance Testing Suite</p>
        </header>

        <div class="grid">
            <!-- Server Info -->
            <div class="card">
                <h2>📡 Server Info</h2>
                <div class="metric">
                    <span class="metric-label">Region</span>
                    <span class="metric-value" id="infoRegion">-</span>
                </div>
                <div class="metric">
                    <span class="metric-label">Location</span>
                    <span class="metric-value" id="infoLocation">-</span>
                </div>
                <div class="metric">
                    <span class="metric-label">Uptime</span>
                    <span class="metric-value" id="infoUptime">-</span>
                </div>
                <div class="metric">
                    <span class="metric-label">Total Requests</span>
                    <span class="metric-value" id="infoRequests">-</span>
                </div>
                <div class="metric">
                    <span class="metric-label">Data Served</span>
                    <span class="metric-value" id="infoBytes">-</span>
                </div>
                <button onclick="refreshInfo()" class="secondary" style="margin-top: 15px">Refresh</button>
            </div>

            <!-- Latency Test -->
            <div class="card">
                <h2>⚡ Latency Test</h2>
                <div class="metric">
                    <span class="metric-label">Current</span>
                    <span class="metric-value" id="latencyCurrent">-</span>
                </div>
                <div class="metric">
                    <span class="metric-label">Average</span>
                    <span class="metric-value" id="latencyAvg">-</span>
                </div>
                <div class="metric">
                    <span class="metric-label">Min / Max</span>
                    <span class="metric-value" id="latencyMinMax">-</span>
                </div>
                <div class="metric">
                    <span class="metric-label">Samples</span>
                    <span class="metric-value" id="latencySamples">0</span>
                </div>
                <div class="latency-chart" id="latencyChart"></div>
                <button onclick="startLatencyTest()" id="latencyBtn">Start Latency Test (50 pings)</button>
            </div>

            <!-- Download Speed -->
            <div class="card">
                <h2>⬇️ Download Speed</h2>
                <div class="speed-display">
                    <div class="speed-value" id="downloadSpeed">-</div>
                    <div class="speed-unit">Mbps</div>
                </div>
                <div class="progress-bar">
                    <div class="progress-fill" id="downloadProgress" style="width: 0%"></div>
                </div>
                <div class="metric">
                    <span class="metric-label">Downloaded</span>
                    <span class="metric-value" id="downloadBytes">-</span>
                </div>
                <div class="metric">
                    <span class="metric-label">Time</span>
                    <span class="metric-value" id="downloadTime">-</span>
                </div>
                <select id="downloadSize" style="width: 100%; padding: 10px; margin: 10px 0; background: #333; color: #fff; border: none; border-radius: 5px;">
                    <option value="1048576">1 MB</option>
                    <option value="5242880">5 MB</option>
                    <option value="10485760" selected>10 MB</option>
                    <option value="26214400">25 MB</option>
                    <option value="52428800">50 MB</option>
                    <option value="104857600">100 MB</option>
                </select>
                <button onclick="startDownloadTest()" id="downloadBtn">Start Download Test</button>
            </div>

            <!-- Upload Speed -->
            <div class="card">
                <h2>⬆️ Upload Speed</h2>
                <div class="speed-display">
                    <div class="speed-value" id="uploadSpeed">-</div>
                    <div class="speed-unit">Mbps</div>
                </div>
                <div class="progress-bar">
                    <div class="progress-fill" id="uploadProgress" style="width: 0%"></div>
                </div>
                <div class="metric">
                    <span class="metric-label">Uploaded</span>
                    <span class="metric-value" id="uploadBytes">-</span>
                </div>
                <div class="metric">
                    <span class="metric-label">Time</span>
                    <span class="metric-value" id="uploadTime">-</span>
                </div>
                <select id="uploadSize" style="width: 100%; padding: 10px; margin: 10px 0; background: #333; color: #fff; border: none; border-radius: 5px;">
                    <option value="1048576">1 MB</option>
                    <option value="5242880" selected>5 MB</option>
                    <option value="10485760">10 MB</option>
                    <option value="26214400">25 MB</option>
                </select>
                <button onclick="startUploadTest()" id="uploadBtn">Start Upload Test</button>
            </div>

            <!-- Stress Test -->
            <div class="card">
                <h2>🔥 Stress Test</h2>
                <div class="metric">
                    <span class="metric-label">Concurrent Requests</span>
                    <span class="metric-value" id="stressRequests">0</span>
                </div>
                <div class="metric">
                    <span class="metric-label">Completed</span>
                    <span class="metric-value" id="stressCompleted">0</span>
                </div>
                <div class="metric">
                    <span class="metric-label">Failed</span>
                    <span class="metric-value" id="stressFailed">0</span>
                </div>
                <div class="metric">
                    <span class="metric-label">Req/sec</span>
                    <span class="metric-value" id="stressRps">-</span>
                </div>
                <div class="progress-bar">
                    <div class="progress-fill" id="stressProgress" style="width: 0%"></div>
                </div>
                <select id="stressCount" style="width: 100%; padding: 10px; margin: 10px 0; background: #333; color: #fff; border: none; border-radius: 5px;">
                    <option value="50">50 requests</option>
                    <option value="100" selected>100 requests</option>
                    <option value="200">200 requests</option>
                    <option value="500">500 requests</option>
                    <option value="1000">1000 requests</option>
                </select>
                <button onclick="startStressTest()" id="stressBtn">Start Stress Test</button>
            </div>

            <!-- Test Log -->
            <div class="card">
                <h2>📋 Test Log</h2>
                <div class="log" id="testLog">
                    <div class="log-entry info">Ready to run tests...</div>
                </div>
                <button onclick="clearLog()" class="secondary" style="margin-top: 10px">Clear Log</button>
                <button onclick="runAllTests()" class="danger" style="margin-top: 5px">Run All Tests</button>
            </div>
        </div>

        <footer>
            edgeProxy v0.1.0 | Performance Benchmark Suite |
            <span id="clientTime"></span>
        </footer>
    </div>

    <script>
        // State
        let latencyResults = [];
        let serverRegion = 'unknown';

        // Utility functions
        function formatBytes(bytes) {
            if (bytes < 1024) return bytes + ' B';
            if (bytes < 1048576) return (bytes / 1024).toFixed(2) + ' KB';
            if (bytes < 1073741824) return (bytes / 1048576).toFixed(2) + ' MB';
            return (bytes / 1073741824).toFixed(2) + ' GB';
        }

        function formatDuration(ms) {
            if (ms < 1000) return ms.toFixed(0) + ' ms';
            return (ms / 1000).toFixed(2) + ' s';
        }

        function log(msg, type = 'info') {
            const logEl = document.getElementById('testLog');
            const entry = document.createElement('div');
            entry.className = 'log-entry ' + type;
            entry.textContent = '[' + new Date().toLocaleTimeString() + '] ' + msg;
            logEl.insertBefore(entry, logEl.firstChild);
        }

        function clearLog() {
            document.getElementById('testLog').innerHTML = '<div class="log-entry info">Log cleared</div>';
        }

        // Refresh server info
        async function refreshInfo() {
            try {
                const res = await fetch('/api/info');
                const data = await res.json();
                serverRegion = data.region;
                document.getElementById('regionBadge').textContent = data.region.toUpperCase() + ' - ' + data.region_name;
                document.getElementById('infoRegion').textContent = data.region.toUpperCase();
                document.getElementById('infoLocation').textContent = data.region_name;
                document.getElementById('infoUptime').textContent = data.uptime_secs + 's';
                document.getElementById('infoRequests').textContent = data.requests.toLocaleString();
                document.getElementById('infoBytes').textContent = formatBytes(data.bytes_served);
                log('Server info refreshed: ' + data.region.toUpperCase(), 'success');
            } catch (e) {
                log('Failed to fetch server info: ' + e.message, 'error');
            }
        }

        // Latency test
        async function startLatencyTest() {
            const btn = document.getElementById('latencyBtn');
            btn.disabled = true;
            btn.textContent = 'Testing...';

            latencyResults = [];
            const chart = document.getElementById('latencyChart');
            chart.innerHTML = '';

            log('Starting latency test (50 pings)...');

            for (let i = 0; i < 50; i++) {
                const start = performance.now();
                try {
                    await fetch('/api/latency?_=' + Date.now());
                    const latency = performance.now() - start;
                    latencyResults.push(latency);

                    // Update UI
                    document.getElementById('latencyCurrent').textContent = latency.toFixed(1) + ' ms';
                    document.getElementById('latencySamples').textContent = latencyResults.length;

                    const avg = latencyResults.reduce((a, b) => a + b, 0) / latencyResults.length;
                    document.getElementById('latencyAvg').textContent = avg.toFixed(1) + ' ms';

                    const min = Math.min(...latencyResults);
                    const max = Math.max(...latencyResults);
                    document.getElementById('latencyMinMax').textContent = min.toFixed(0) + ' / ' + max.toFixed(0) + ' ms';

                    // Add bar to chart
                    const bar = document.createElement('div');
                    bar.className = 'latency-bar';
                    if (latency > 100) bar.className += ' high';
                    if (latency > 200) bar.className += ' very-high';
                    bar.style.height = Math.min(latency, 100) + '%';
                    chart.appendChild(bar);

                } catch (e) {
                    log('Ping failed: ' + e.message, 'error');
                }

                await new Promise(r => setTimeout(r, 50));
            }

            const finalAvg = latencyResults.reduce((a, b) => a + b, 0) / latencyResults.length;
            log('Latency test complete. Avg: ' + finalAvg.toFixed(1) + ' ms', 'success');

            btn.disabled = false;
            btn.textContent = 'Start Latency Test (50 pings)';
        }

        // Download test
        async function startDownloadTest() {
            const btn = document.getElementById('downloadBtn');
            const size = parseInt(document.getElementById('downloadSize').value);

            btn.disabled = true;
            btn.textContent = 'Downloading...';
            document.getElementById('downloadProgress').style.width = '0%';
            document.getElementById('downloadSpeed').textContent = '-';

            log('Starting download test (' + formatBytes(size) + ')...');

            try {
                const start = performance.now();
                const res = await fetch('/api/download?size=' + size);

                const reader = res.body.getReader();
                let received = 0;

                while (true) {
                    const { done, value } = await reader.read();
                    if (done) break;
                    received += value.length;

                    const progress = (received / size * 100);
                    document.getElementById('downloadProgress').style.width = progress + '%';
                    document.getElementById('downloadProgress').textContent = progress.toFixed(0) + '%';

                    const elapsed = (performance.now() - start) / 1000;
                    const speed = (received * 8 / elapsed / 1024 / 1024);
                    document.getElementById('downloadSpeed').textContent = speed.toFixed(2);
                    document.getElementById('downloadBytes').textContent = formatBytes(received);
                    document.getElementById('downloadTime').textContent = formatDuration(elapsed * 1000);
                }

                const totalTime = performance.now() - start;
                const finalSpeed = (received * 8 / (totalTime / 1000) / 1024 / 1024);
                document.getElementById('downloadSpeed').textContent = finalSpeed.toFixed(2);

                log('Download complete: ' + formatBytes(received) + ' in ' + formatDuration(totalTime) + ' (' + finalSpeed.toFixed(2) + ' Mbps)', 'success');

            } catch (e) {
                log('Download failed: ' + e.message, 'error');
            }

            btn.disabled = false;
            btn.textContent = 'Start Download Test';
        }

        // Upload test
        async function startUploadTest() {
            const btn = document.getElementById('uploadBtn');
            const size = parseInt(document.getElementById('uploadSize').value);

            btn.disabled = true;
            btn.textContent = 'Uploading...';
            document.getElementById('uploadProgress').style.width = '0%';
            document.getElementById('uploadSpeed').textContent = '-';

            log('Starting upload test (' + formatBytes(size) + ')...');

            try {
                // Generate random data
                const data = new Uint8Array(size);
                crypto.getRandomValues(data);

                document.getElementById('uploadProgress').style.width = '50%';
                document.getElementById('uploadProgress').textContent = 'Uploading...';

                const start = performance.now();
                const res = await fetch('/api/upload', {
                    method: 'POST',
                    body: data
                });
                const totalTime = performance.now() - start;

                document.getElementById('uploadProgress').style.width = '100%';
                document.getElementById('uploadProgress').textContent = '100%';

                const result = await res.json();
                const speed = (size * 8 / (totalTime / 1000) / 1024 / 1024);

                document.getElementById('uploadSpeed').textContent = speed.toFixed(2);
                document.getElementById('uploadBytes').textContent = formatBytes(size);
                document.getElementById('uploadTime').textContent = formatDuration(totalTime);

                log('Upload complete: ' + formatBytes(size) + ' in ' + formatDuration(totalTime) + ' (' + speed.toFixed(2) + ' Mbps)', 'success');

            } catch (e) {
                log('Upload failed: ' + e.message, 'error');
            }

            btn.disabled = false;
            btn.textContent = 'Start Upload Test';
        }

        // Stress test
        async function startStressTest() {
            const btn = document.getElementById('stressBtn');
            const count = parseInt(document.getElementById('stressCount').value);

            btn.disabled = true;
            btn.textContent = 'Running...';

            let completed = 0;
            let failed = 0;
            const start = performance.now();

            log('Starting stress test (' + count + ' concurrent requests)...');

            document.getElementById('stressRequests').textContent = count;
            document.getElementById('stressCompleted').textContent = '0';
            document.getElementById('stressFailed').textContent = '0';
            document.getElementById('stressProgress').style.width = '0%';

            const promises = [];
            for (let i = 0; i < count; i++) {
                promises.push(
                    fetch('/api/latency?_=' + Date.now() + '_' + i)
                        .then(() => {
                            completed++;
                            document.getElementById('stressCompleted').textContent = completed;
                            const progress = ((completed + failed) / count * 100);
                            document.getElementById('stressProgress').style.width = progress + '%';
                            document.getElementById('stressProgress').textContent = progress.toFixed(0) + '%';

                            const elapsed = (performance.now() - start) / 1000;
                            const rps = completed / elapsed;
                            document.getElementById('stressRps').textContent = rps.toFixed(1);
                        })
                        .catch(() => {
                            failed++;
                            document.getElementById('stressFailed').textContent = failed;
                        })
                );
            }

            await Promise.all(promises);

            const totalTime = performance.now() - start;
            const rps = completed / (totalTime / 1000);

            log('Stress test complete: ' + completed + '/' + count + ' successful, ' + rps.toFixed(1) + ' req/sec',
                failed > 0 ? 'warning' : 'success');

            btn.disabled = false;
            btn.textContent = 'Start Stress Test';
        }

        // Run all tests
        async function runAllTests() {
            log('=== Running all tests ===', 'info');
            await refreshInfo();
            await startLatencyTest();
            await startDownloadTest();
            await startUploadTest();
            await startStressTest();
            log('=== All tests complete ===', 'success');
        }

        // Initialize
        refreshInfo();
        setInterval(() => {
            document.getElementById('clientTime').textContent = new Date().toLocaleString();
        }, 1000);
        document.getElementById('clientTime').textContent = new Date().toLocaleString();
    </script>
</body>
</html>
`
