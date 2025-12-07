package main

import (
	"database/sql"
	"encoding/json"
	"fmt"
	"log"
	"net/http"
	"os"
	"strconv"
	"time"

	_ "github.com/lib/pq"
)

type Contact struct {
	ID        int       `json:"id"`
	Name      string    `json:"name"`
	Email     string    `json:"email"`
	Phone     *string   `json:"phone,omitempty"`
	Company   *string   `json:"company,omitempty"`
	Notes     *string   `json:"notes,omitempty"`
	CreatedAt time.Time `json:"created_at"`
	UpdatedAt time.Time `json:"updated_at"`
}

type ContactInput struct {
	Name    string  `json:"name"`
	Email   string  `json:"email"`
	Phone   *string `json:"phone,omitempty"`
	Company *string `json:"company,omitempty"`
	Notes   *string `json:"notes,omitempty"`
}

type HealthResponse struct {
	Status   string `json:"status"`
	Database string `json:"database"`
	Region   string `json:"region"`
	DBHost   string `json:"db_host"`
}

type StatsResponse struct {
	TotalContacts   int       `json:"total_contacts"`
	UniqueCompanies int       `json:"unique_companies"`
	LatestContact   *time.Time `json:"latest_contact,omitempty"`
	ServedBy        string    `json:"served_by"`
	DBHost          string    `json:"db_host"`
}

var db *sql.DB

func getEnv(key, defaultValue string) string {
	if value := os.Getenv(key); value != "" {
		return value
	}
	return defaultValue
}

func initDB() error {
	dbHost := getEnv("DB_HOST", "localhost")
	dbPort := getEnv("DB_PORT", "5432")
	dbUser := getEnv("DB_USER", "postgres")
	dbPassword := getEnv("DB_PASSWORD", "")
	dbName := getEnv("DB_NAME", "contacts")

	connStr := fmt.Sprintf("host=%s port=%s user=%s password=%s dbname=%s sslmode=require",
		dbHost, dbPort, dbUser, dbPassword, dbName)

	var err error
	db, err = sql.Open("postgres", connStr)
	if err != nil {
		return err
	}

	db.SetMaxOpenConns(10)
	db.SetMaxIdleConns(5)
	db.SetConnMaxLifetime(time.Minute * 5)

	return db.Ping()
}

func initSchema() error {
	schema := `
	CREATE TABLE IF NOT EXISTS contacts (
		id SERIAL PRIMARY KEY,
		name VARCHAR(255) NOT NULL,
		email VARCHAR(255) NOT NULL,
		phone VARCHAR(50),
		company VARCHAR(255),
		notes TEXT,
		created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
		updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
	);
	CREATE INDEX IF NOT EXISTS idx_contacts_name ON contacts(name);
	CREATE INDEX IF NOT EXISTS idx_contacts_email ON contacts(email);
	CREATE INDEX IF NOT EXISTS idx_contacts_company ON contacts(company);
	`
	_, err := db.Exec(schema)
	return err
}

func jsonResponse(w http.ResponseWriter, data interface{}, statusCode int) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(statusCode)
	json.NewEncoder(w).Encode(data)
}

func errorResponse(w http.ResponseWriter, message string, statusCode int) {
	jsonResponse(w, map[string]string{"error": message}, statusCode)
}

func healthHandler(w http.ResponseWriter, r *http.Request) {
	region := getEnv("FLY_REGION", "local")
	dbHost := getEnv("DB_HOST", "localhost")

	resp := HealthResponse{
		Status:   "healthy",
		Database: "connected",
		Region:   region,
		DBHost:   dbHost,
	}

	if err := db.Ping(); err != nil {
		resp.Status = "unhealthy"
		resp.Database = err.Error()
	}

	jsonResponse(w, resp, http.StatusOK)
}

func rootHandler(w http.ResponseWriter, r *http.Request) {
	hostname, _ := os.Hostname()
	jsonResponse(w, map[string]interface{}{
		"service":   "contacts-api",
		"version":   "1.0.0",
		"hostname":  hostname,
		"region":    getEnv("FLY_REGION", "local"),
		"timestamp": time.Now().UTC().Format(time.RFC3339),
	}, http.StatusOK)
}

func listContactsHandler(w http.ResponseWriter, r *http.Request) {
	limit, _ := strconv.Atoi(r.URL.Query().Get("limit"))
	if limit <= 0 || limit > 1000 {
		limit = 100
	}
	offset, _ := strconv.Atoi(r.URL.Query().Get("offset"))

	rows, err := db.Query(`
		SELECT id, name, email, phone, company, notes, created_at, updated_at
		FROM contacts ORDER BY name LIMIT $1 OFFSET $2
	`, limit, offset)
	if err != nil {
		errorResponse(w, err.Error(), http.StatusInternalServerError)
		return
	}
	defer rows.Close()

	contacts := []Contact{}
	for rows.Next() {
		var c Contact
		err := rows.Scan(&c.ID, &c.Name, &c.Email, &c.Phone, &c.Company, &c.Notes, &c.CreatedAt, &c.UpdatedAt)
		if err != nil {
			continue
		}
		contacts = append(contacts, c)
	}

	var total int
	db.QueryRow("SELECT COUNT(*) FROM contacts").Scan(&total)

	jsonResponse(w, map[string]interface{}{
		"contacts":  contacts,
		"total":     total,
		"limit":     limit,
		"offset":    offset,
		"served_by": getEnv("FLY_REGION", "local"),
	}, http.StatusOK)
}

func getContactHandler(w http.ResponseWriter, r *http.Request) {
	idStr := r.URL.Path[len("/contacts/"):]
	id, err := strconv.Atoi(idStr)
	if err != nil {
		errorResponse(w, "Invalid contact ID", http.StatusBadRequest)
		return
	}

	var c Contact
	err = db.QueryRow(`
		SELECT id, name, email, phone, company, notes, created_at, updated_at
		FROM contacts WHERE id = $1
	`, id).Scan(&c.ID, &c.Name, &c.Email, &c.Phone, &c.Company, &c.Notes, &c.CreatedAt, &c.UpdatedAt)
	if err == sql.ErrNoRows {
		errorResponse(w, "Contact not found", http.StatusNotFound)
		return
	}
	if err != nil {
		errorResponse(w, err.Error(), http.StatusInternalServerError)
		return
	}

	jsonResponse(w, c, http.StatusOK)
}

func createContactHandler(w http.ResponseWriter, r *http.Request) {
	var input ContactInput
	if err := json.NewDecoder(r.Body).Decode(&input); err != nil {
		errorResponse(w, "Invalid JSON", http.StatusBadRequest)
		return
	}

	if input.Name == "" || input.Email == "" {
		errorResponse(w, "Name and email are required", http.StatusBadRequest)
		return
	}

	var c Contact
	err := db.QueryRow(`
		INSERT INTO contacts (name, email, phone, company, notes)
		VALUES ($1, $2, $3, $4, $5)
		RETURNING id, name, email, phone, company, notes, created_at, updated_at
	`, input.Name, input.Email, input.Phone, input.Company, input.Notes).Scan(
		&c.ID, &c.Name, &c.Email, &c.Phone, &c.Company, &c.Notes, &c.CreatedAt, &c.UpdatedAt)
	if err != nil {
		errorResponse(w, err.Error(), http.StatusInternalServerError)
		return
	}

	jsonResponse(w, c, http.StatusCreated)
}

func updateContactHandler(w http.ResponseWriter, r *http.Request) {
	idStr := r.URL.Path[len("/contacts/"):]
	id, err := strconv.Atoi(idStr)
	if err != nil {
		errorResponse(w, "Invalid contact ID", http.StatusBadRequest)
		return
	}

	var input ContactInput
	if err := json.NewDecoder(r.Body).Decode(&input); err != nil {
		errorResponse(w, "Invalid JSON", http.StatusBadRequest)
		return
	}

	var c Contact
	err = db.QueryRow(`
		UPDATE contacts
		SET name = $1, email = $2, phone = $3, company = $4, notes = $5, updated_at = CURRENT_TIMESTAMP
		WHERE id = $6
		RETURNING id, name, email, phone, company, notes, created_at, updated_at
	`, input.Name, input.Email, input.Phone, input.Company, input.Notes, id).Scan(
		&c.ID, &c.Name, &c.Email, &c.Phone, &c.Company, &c.Notes, &c.CreatedAt, &c.UpdatedAt)
	if err == sql.ErrNoRows {
		errorResponse(w, "Contact not found", http.StatusNotFound)
		return
	}
	if err != nil {
		errorResponse(w, err.Error(), http.StatusInternalServerError)
		return
	}

	jsonResponse(w, c, http.StatusOK)
}

func deleteContactHandler(w http.ResponseWriter, r *http.Request) {
	idStr := r.URL.Path[len("/contacts/"):]
	id, err := strconv.Atoi(idStr)
	if err != nil {
		errorResponse(w, "Invalid contact ID", http.StatusBadRequest)
		return
	}

	result, err := db.Exec("DELETE FROM contacts WHERE id = $1", id)
	if err != nil {
		errorResponse(w, err.Error(), http.StatusInternalServerError)
		return
	}

	rows, _ := result.RowsAffected()
	if rows == 0 {
		errorResponse(w, "Contact not found", http.StatusNotFound)
		return
	}

	w.WriteHeader(http.StatusNoContent)
}

func searchContactsHandler(w http.ResponseWriter, r *http.Request) {
	query := r.URL.Path[len("/contacts/search/"):]
	if query == "" {
		errorResponse(w, "Query is required", http.StatusBadRequest)
		return
	}

	searchTerm := "%" + query + "%"
	rows, err := db.Query(`
		SELECT id, name, email, phone, company, notes, created_at, updated_at
		FROM contacts
		WHERE name ILIKE $1 OR email ILIKE $1 OR company ILIKE $1
		ORDER BY name LIMIT 50
	`, searchTerm)
	if err != nil {
		errorResponse(w, err.Error(), http.StatusInternalServerError)
		return
	}
	defer rows.Close()

	contacts := []Contact{}
	for rows.Next() {
		var c Contact
		err := rows.Scan(&c.ID, &c.Name, &c.Email, &c.Phone, &c.Company, &c.Notes, &c.CreatedAt, &c.UpdatedAt)
		if err != nil {
			continue
		}
		contacts = append(contacts, c)
	}

	jsonResponse(w, map[string]interface{}{
		"contacts": contacts,
		"query":    query,
		"count":    len(contacts),
	}, http.StatusOK)
}

func statsHandler(w http.ResponseWriter, r *http.Request) {
	var total, companies int
	var latest *time.Time

	db.QueryRow("SELECT COUNT(*) FROM contacts").Scan(&total)
	db.QueryRow("SELECT COUNT(DISTINCT company) FROM contacts WHERE company IS NOT NULL").Scan(&companies)

	var latestTime time.Time
	err := db.QueryRow("SELECT created_at FROM contacts ORDER BY created_at DESC LIMIT 1").Scan(&latestTime)
	if err == nil {
		latest = &latestTime
	}

	jsonResponse(w, StatsResponse{
		TotalContacts:   total,
		UniqueCompanies: companies,
		LatestContact:   latest,
		ServedBy:        getEnv("FLY_REGION", "local"),
		DBHost:          getEnv("DB_HOST", "localhost"),
	}, http.StatusOK)
}

// Benchmark handlers for RDS latency testing
type BenchmarkResult struct {
	Operation    string  `json:"operation"`
	LatencyMs    float64 `json:"latency_ms"`
	Region       string  `json:"region"`
	DBHost       string  `json:"db_host"`
	Success      bool    `json:"success"`
	Error        string  `json:"error,omitempty"`
	RowsAffected int64   `json:"rows_affected,omitempty"`
}

func benchmarkReadHandler(w http.ResponseWriter, r *http.Request) {
	start := time.Now()
	region := getEnv("FLY_REGION", "local")
	dbHost := getEnv("DB_HOST", "localhost")

	var count int
	err := db.QueryRow("SELECT COUNT(*) FROM contacts").Scan(&count)
	latency := float64(time.Since(start).Microseconds()) / 1000.0

	result := BenchmarkResult{
		Operation: "READ",
		LatencyMs: latency,
		Region:    region,
		DBHost:    dbHost,
		Success:   err == nil,
	}
	if err != nil {
		result.Error = err.Error()
	}

	jsonResponse(w, result, http.StatusOK)
}

func benchmarkInsertHandler(w http.ResponseWriter, r *http.Request) {
	start := time.Now()
	region := getEnv("FLY_REGION", "local")
	dbHost := getEnv("DB_HOST", "localhost")

	// Insert a benchmark record
	name := fmt.Sprintf("Benchmark-%s-%d", region, time.Now().UnixNano())
	email := fmt.Sprintf("bench-%d@test.local", time.Now().UnixNano())

	result, err := db.Exec(`
		INSERT INTO contacts (name, email, notes)
		VALUES ($1, $2, $3)
	`, name, email, "Benchmark test record")

	latency := float64(time.Since(start).Microseconds()) / 1000.0

	benchResult := BenchmarkResult{
		Operation: "INSERT",
		LatencyMs: latency,
		Region:    region,
		DBHost:    dbHost,
		Success:   err == nil,
	}
	if err != nil {
		benchResult.Error = err.Error()
	} else {
		rows, _ := result.RowsAffected()
		benchResult.RowsAffected = rows
	}

	jsonResponse(w, benchResult, http.StatusOK)
}

func benchmarkFullHandler(w http.ResponseWriter, r *http.Request) {
	region := getEnv("FLY_REGION", "local")
	dbHost := getEnv("DB_HOST", "localhost")

	iterations := 10
	if iter := r.URL.Query().Get("iterations"); iter != "" {
		if n, err := strconv.Atoi(iter); err == nil && n > 0 && n <= 100 {
			iterations = n
		}
	}

	type FullBenchmark struct {
		Region       string    `json:"region"`
		DBHost       string    `json:"db_host"`
		Iterations   int       `json:"iterations"`
		ReadAvgMs    float64   `json:"read_avg_ms"`
		ReadMinMs    float64   `json:"read_min_ms"`
		ReadMaxMs    float64   `json:"read_max_ms"`
		InsertAvgMs  float64   `json:"insert_avg_ms"`
		InsertMinMs  float64   `json:"insert_min_ms"`
		InsertMaxMs  float64   `json:"insert_max_ms"`
		ReadLatencies  []float64 `json:"read_latencies"`
		InsertLatencies []float64 `json:"insert_latencies"`
		Timestamp    string    `json:"timestamp"`
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

	result := FullBenchmark{
		Region:          region,
		DBHost:          dbHost,
		Iterations:      iterations,
		ReadAvgMs:       readAvg,
		ReadMinMs:       readMin,
		ReadMaxMs:       readMax,
		InsertAvgMs:     insertAvg,
		InsertMinMs:     insertMin,
		InsertMaxMs:     insertMax,
		ReadLatencies:   readLatencies,
		InsertLatencies: insertLatencies,
		Timestamp:       time.Now().UTC().Format(time.RFC3339),
	}

	jsonResponse(w, result, http.StatusOK)
}

func cleanupBenchmarkHandler(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodDelete {
		errorResponse(w, "Method not allowed", http.StatusMethodNotAllowed)
		return
	}

	result, err := db.Exec("DELETE FROM contacts WHERE notes = 'Benchmark test record' OR notes = 'Benchmark'")
	if err != nil {
		errorResponse(w, err.Error(), http.StatusInternalServerError)
		return
	}

	rows, _ := result.RowsAffected()
	jsonResponse(w, map[string]interface{}{
		"deleted": rows,
		"message": "Benchmark records cleaned up",
	}, http.StatusOK)
}

func contactsRouter(w http.ResponseWriter, r *http.Request) {
	path := r.URL.Path

	// Handle search
	if len(path) > 17 && path[:17] == "/contacts/search/" {
		if r.Method == http.MethodGet {
			searchContactsHandler(w, r)
			return
		}
	}

	// Handle specific contact
	if len(path) > 10 && path[:10] == "/contacts/" {
		switch r.Method {
		case http.MethodGet:
			getContactHandler(w, r)
		case http.MethodPut:
			updateContactHandler(w, r)
		case http.MethodDelete:
			deleteContactHandler(w, r)
		default:
			errorResponse(w, "Method not allowed", http.StatusMethodNotAllowed)
		}
		return
	}

	// Handle /contacts
	switch r.Method {
	case http.MethodGet:
		listContactsHandler(w, r)
	case http.MethodPost:
		createContactHandler(w, r)
	default:
		errorResponse(w, "Method not allowed", http.StatusMethodNotAllowed)
	}
}

func main() {
	log.Println("Initializing Contacts API...")

	if err := initDB(); err != nil {
		log.Fatalf("Failed to connect to database: %v", err)
	}
	log.Println("Database connected")

	if err := initSchema(); err != nil {
		log.Fatalf("Failed to initialize schema: %v", err)
	}
	log.Println("Schema initialized")

	http.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/" {
			rootHandler(w, r)
			return
		}
		errorResponse(w, "Not found", http.StatusNotFound)
	})
	http.HandleFunc("/health", healthHandler)
	http.HandleFunc("/stats", statsHandler)
	http.HandleFunc("/contacts", contactsRouter)
	http.HandleFunc("/contacts/", contactsRouter)

	// Benchmark endpoints
	http.HandleFunc("/benchmark/read", benchmarkReadHandler)
	http.HandleFunc("/benchmark/insert", benchmarkInsertHandler)
	http.HandleFunc("/benchmark/full", benchmarkFullHandler)
	http.HandleFunc("/benchmark/cleanup", cleanupBenchmarkHandler)

	port := getEnv("PORT", "8080")
	log.Printf("Server starting on port %s", port)
	log.Fatal(http.ListenAndServe(":"+port, nil))
}
