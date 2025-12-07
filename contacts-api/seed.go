// +build ignore

package main

import (
	"database/sql"
	"fmt"
	"log"
	"math/rand"
	"os"
	"time"

	_ "github.com/lib/pq"
)

var firstNames = []string{
	"Ana", "Pedro", "Maria", "João", "Carla", "Lucas", "Fernanda", "Rafael",
	"Juliana", "Bruno", "Camila", "Diego", "Beatriz", "Thiago", "Amanda",
	"Gustavo", "Larissa", "Rodrigo", "Patricia", "Leonardo", "Mariana",
	"Felipe", "Isabela", "Daniel", "Natalia", "Eduardo", "Carolina", "Andre",
	"Gabriela", "Ricardo", "Vanessa", "Marcos", "Leticia", "Paulo", "Renata",
	"James", "Emma", "Michael", "Sophia", "William", "Olivia", "David", "Ava",
	"Hans", "Greta", "Klaus", "Ingrid", "François", "Marie", "Pierre", "Claire",
}

var lastNames = []string{
	"Silva", "Santos", "Oliveira", "Souza", "Lima", "Pereira", "Ferreira",
	"Almeida", "Costa", "Rodrigues", "Martins", "Araujo", "Carvalho", "Gomes",
	"Nascimento", "Ribeiro", "Barros", "Barbosa", "Moreira", "Melo", "Cardoso",
	"Lopes", "Mendes", "Dias", "Ramos", "Vieira", "Nunes", "Monteiro", "Pinto",
	"Smith", "Johnson", "Williams", "Brown", "Jones", "Garcia", "Miller",
	"Mueller", "Schmidt", "Weber", "Dubois", "Martin", "Bernard", "Petit",
}

var companies = []string{
	"TechCorp Brasil", "Innovate Solutions", "Digital Masters", "Cloud Nine Tech",
	"DataFlow Systems", "Smart Logic", "ByteWise", "CodeCraft", "DevOps Pro",
	"Agile Works", "Startup Hub", "FinTech Solutions", "E-Commerce Plus",
	"Mobile First", "AI Dynamics", "Cyber Security SA", "Big Data Analytics",
	"IoT Innovations", "Blockchain Labs", "SaaS Platform", "API Gateway Inc",
	"Microservices Ltd", "Container World", "Kubernetes Masters", "AWS Partners",
	"Google Cloud Team", "Azure Experts", "DevSecOps Group", "Terraform Co",
	"GitLab Solutions", "GitHub Enterprise", "CI/CD Pipeline", "Monitoring Pro",
}

var domains = []string{
	"gmail.com", "outlook.com", "yahoo.com", "hotmail.com", "icloud.com",
	"protonmail.com", "empresa.com.br", "corporativo.com", "tech.io",
}

var phoneFormats = []string{
	"+55 11 9%d%d%d%d-%d%d%d%d",
	"+55 21 9%d%d%d%d-%d%d%d%d",
	"+1 555 %d%d%d-%d%d%d%d",
	"+44 20 %d%d%d%d %d%d%d%d",
	"+49 30 %d%d%d%d%d%d%d%d",
}

func randomPhone() string {
	format := phoneFormats[rand.Intn(len(phoneFormats))]
	digits := make([]interface{}, 8)
	for i := range digits {
		digits[i] = rand.Intn(10)
	}
	return fmt.Sprintf(format, digits...)
}

func randomEmail(firstName, lastName string) string {
	domain := domains[rand.Intn(len(domains))]
	formats := []string{
		"%s.%s@%s",
		"%s%s@%s",
		"%s_%s@%s",
	}
	format := formats[rand.Intn(len(formats))]
	return fmt.Sprintf(format, firstName, lastName, domain)
}

func randomNotes() *string {
	notes := []string{
		"Cliente VIP - prioridade alta",
		"Prefere contato por email",
		"Reunião agendada para próxima semana",
		"Interessado em novos produtos",
		"Parceiro estratégico",
		"Lead qualificado",
		"Aguardando proposta comercial",
		"Contato referenciado por outro cliente",
		"Participou do último evento",
		"Potencial para upsell",
	}
	if rand.Float32() > 0.5 {
		note := notes[rand.Intn(len(notes))]
		return &note
	}
	return nil
}

func getEnv(key, defaultValue string) string {
	if value := os.Getenv(key); value != "" {
		return value
	}
	return defaultValue
}

func main() {
	rand.Seed(time.Now().UnixNano())

	dbHost := getEnv("DB_HOST", "localhost")
	dbPort := getEnv("DB_PORT", "5432")
	dbUser := getEnv("DB_USER", "postgres")
	dbPassword := getEnv("DB_PASSWORD", "")
	dbName := getEnv("DB_NAME", "contacts")
	count := 500

	connStr := fmt.Sprintf("host=%s port=%s user=%s password=%s dbname=%s sslmode=require",
		dbHost, dbPort, dbUser, dbPassword, dbName)

	db, err := sql.Open("postgres", connStr)
	if err != nil {
		log.Fatalf("Failed to connect: %v", err)
	}
	defer db.Close()

	// Create table if not exists
	_, err = db.Exec(`
		CREATE TABLE IF NOT EXISTS contacts (
			id SERIAL PRIMARY KEY,
			name VARCHAR(255) NOT NULL,
			email VARCHAR(255) NOT NULL,
			phone VARCHAR(50),
			company VARCHAR(255),
			notes TEXT,
			created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
			updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
		)
	`)
	if err != nil {
		log.Fatalf("Failed to create table: %v", err)
	}

	log.Printf("Seeding %d contacts...", count)

	for i := 0; i < count; i++ {
		firstName := firstNames[rand.Intn(len(firstNames))]
		lastName := lastNames[rand.Intn(len(lastNames))]
		name := firstName + " " + lastName
		email := randomEmail(firstName, lastName)
		phone := randomPhone()
		company := companies[rand.Intn(len(companies))]
		notes := randomNotes()

		_, err := db.Exec(`
			INSERT INTO contacts (name, email, phone, company, notes)
			VALUES ($1, $2, $3, $4, $5)
		`, name, email, phone, company, notes)
		if err != nil {
			log.Printf("Error inserting contact %d: %v", i, err)
			continue
		}

		if (i+1)%100 == 0 {
			log.Printf("Inserted %d contacts...", i+1)
		}
	}

	var total int
	db.QueryRow("SELECT COUNT(*) FROM contacts").Scan(&total)
	log.Printf("Done! Total contacts in database: %d", total)
}
