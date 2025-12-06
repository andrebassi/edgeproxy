#!/usr/bin/env python3
"""
Mock TCP Backend Server
Simulates a backend that responds with its identity
"""

import socket
import sys
import threading
import time

def handle_client(conn, addr, backend_id, region):
    """Handle incoming connection"""
    try:
        # Send identity on connect
        welcome = f"Backend: {backend_id} | Region: {region} | Your IP: {addr[0]}:{addr[1]}\n"
        conn.send(welcome.encode())

        # Echo loop
        while True:
            data = conn.recv(1024)
            if not data:
                break
            response = f"[{backend_id}] Echo: {data.decode().strip()}\n"
            conn.send(response.encode())
    except Exception as e:
        print(f"[{backend_id}] Connection error: {e}")
    finally:
        conn.close()
        print(f"[{backend_id}] Client {addr} disconnected")

def start_backend(host, port, backend_id, region):
    """Start a mock backend server"""
    server = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    server.bind((host, port))
    server.listen(5)

    print(f"[{backend_id}] Backend listening on {host}:{port} (region={region})")

    while True:
        conn, addr = server.accept()
        print(f"[{backend_id}] Connection from {addr}")
        thread = threading.Thread(target=handle_client, args=(conn, addr, backend_id, region))
        thread.daemon = True
        thread.start()

if __name__ == "__main__":
    if len(sys.argv) != 4:
        print("Usage: python mock_backend.py <port> <backend_id> <region>")
        print("Example: python mock_backend.py 9001 sa-node-1 sa")
        sys.exit(1)

    port = int(sys.argv[1])
    backend_id = sys.argv[2]
    region = sys.argv[3]

    # Use 0.0.0.0 to listen on all interfaces (needed for Docker)
    host = "0.0.0.0"
    start_backend(host, port, backend_id, region)
