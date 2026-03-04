//! Minimal HTTP health check binary for distroless Docker containers.
//!
//! Usage: healthcheck <url>
//! Exits 0 if HTTP 200, exits 1 otherwise.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::process;

fn main() {
    let url = match std::env::args().nth(1) {
        Some(u) => u,
        None => {
            eprintln!("Usage: healthcheck <url>");
            process::exit(1);
        }
    };

    // Parse URL: http://host:port/path
    let stripped = url.strip_prefix("http://").unwrap_or_else(|| {
        eprintln!("Only http:// URLs supported");
        process::exit(1);
    });

    let (host_port, path) = match stripped.find('/') {
        Some(i) => (&stripped[..i], &stripped[i..]),
        None => (stripped, "/"),
    };

    let stream = TcpStream::connect(host_port).unwrap_or_else(|e| {
        eprintln!("Connection failed: {}", e);
        process::exit(1);
    });
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .ok();

    let mut stream = stream;
    let request = format!(
        "GET {} HTTP/1.0\r\nHost: {}\r\nConnection: close\r\n\r\n",
        path, host_port
    );
    stream.write_all(request.as_bytes()).unwrap_or_else(|e| {
        eprintln!("Write failed: {}", e);
        process::exit(1);
    });

    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap_or_else(|e| {
        eprintln!("Read failed: {}", e);
        process::exit(1);
    });

    // Check for HTTP 200
    if response.starts_with("HTTP/1.0 200") || response.starts_with("HTTP/1.1 200") {
        process::exit(0);
    } else {
        let status_line = response.lines().next().unwrap_or("(empty response)");
        eprintln!("Unhealthy: {}", status_line);
        process::exit(1);
    }
}
