// Mock Backend Server for edgeProxy Testing
//
// A simple TCP/HTTP server that responds with region and connection info.
// Used to test geo-routing and load balancing in edgeProxy.
//
// Usage:
//   go run main.go -port 9001 -region eu -id mock-eu-1
//   go run main.go -port 9002 -region eu -id mock-eu-2
//   go run main.go -port 9003 -region us -id mock-us-1

package main

import (
	"encoding/json"
	"flag"
	"fmt"
	"log"
	"net/http"
	"os"
	"sync/atomic"
	"time"
)

var (
	port      string
	region    string
	backendID string
	hostname  string

	requestCount uint64
	startTime    time.Time
)

type Response struct {
	BackendID    string `json:"backend_id"`
	Region       string `json:"region"`
	Hostname     string `json:"hostname"`
	Port         string `json:"port"`
	RequestCount uint64 `json:"request_count"`
	UptimeSecs   int    `json:"uptime_secs"`
	Timestamp    string `json:"timestamp"`
	Message      string `json:"message"`
}

func main() {
	flag.StringVar(&port, "port", "9001", "Port to listen on")
	flag.StringVar(&region, "region", "eu", "Region identifier (eu, us, sa, ap)")
	flag.StringVar(&backendID, "id", "", "Backend ID (default: mock-{region}-{port})")
	flag.Parse()

	// Default backend ID
	if backendID == "" {
		backendID = fmt.Sprintf("mock-%s-%s", region, port)
	}

	// Get hostname
	hostname, _ = os.Hostname()
	startTime = time.Now()

	// Routes
	http.HandleFunc("/", handleRoot)
	http.HandleFunc("/health", handleHealth)
	http.HandleFunc("/api/info", handleInfo)
	http.HandleFunc("/api/latency", handleLatency)

	addr := ":" + port
	log.Printf("Mock backend starting: id=%s region=%s port=%s", backendID, region, port)
	log.Printf("Endpoints: / /health /api/info /api/latency")

	if err := http.ListenAndServe(addr, nil); err != nil {
		log.Fatalf("Failed to start server: %v", err)
	}
}

func handleRoot(w http.ResponseWriter, r *http.Request) {
	count := atomic.AddUint64(&requestCount, 1)

	w.Header().Set("Content-Type", "text/plain; charset=utf-8")
	w.Header().Set("X-Backend-ID", backendID)
	w.Header().Set("X-Region", region)

	response := fmt.Sprintf(`
=====================================
  edgeProxy Mock Backend
=====================================

  Backend ID:  %s
  Region:      %s
  Hostname:    %s
  Port:        %s
  Request #:   %d
  Uptime:      %ds

  Client IP:   %s
  Timestamp:   %s

=====================================
`, backendID, region, hostname, port, count,
		int(time.Since(startTime).Seconds()),
		r.RemoteAddr,
		time.Now().UTC().Format(time.RFC3339))

	fmt.Fprint(w, response)
}

func handleHealth(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Content-Type", "text/plain")
	w.Header().Set("X-Backend-ID", backendID)
	w.Header().Set("X-Region", region)
	w.WriteHeader(http.StatusOK)
	fmt.Fprintf(w, "OK - %s (%s)", backendID, region)
}

func handleInfo(w http.ResponseWriter, r *http.Request) {
	count := atomic.AddUint64(&requestCount, 1)

	w.Header().Set("Content-Type", "application/json")
	w.Header().Set("X-Backend-ID", backendID)
	w.Header().Set("X-Region", region)

	resp := Response{
		BackendID:    backendID,
		Region:       region,
		Hostname:     hostname,
		Port:         port,
		RequestCount: count,
		UptimeSecs:   int(time.Since(startTime).Seconds()),
		Timestamp:    time.Now().UTC().Format(time.RFC3339),
		Message:      "Hello from mock backend!",
	}

	json.NewEncoder(w).Encode(resp)
}

func handleLatency(w http.ResponseWriter, r *http.Request) {
	atomic.AddUint64(&requestCount, 1)

	w.Header().Set("Content-Type", "application/json")
	w.Header().Set("X-Backend-ID", backendID)
	w.Header().Set("X-Region", region)

	json.NewEncoder(w).Encode(map[string]interface{}{
		"backend_id": backendID,
		"region":     region,
		"ts":         time.Now().UnixNano(),
	})
}
