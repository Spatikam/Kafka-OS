// kernel/src/net/http.rs
// Minimal HTTP/1.1 Client for Kafka-OS
//
// Built on top of our TCP stack. Enough to make GET requests
// and read responses from real web servers.

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;

use super::tcp;
use super::ip::Ipv4Packet;

/// HTTP Response (parsed).
#[derive(Debug)]
pub struct HttpResponse {
    pub status_code: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

/// Perform an HTTP GET request.
///
/// `ip`: IP address of the server (resolve with dns::resolve first)
/// `host`: Hostname for the Host header (e.g., "example.com")
/// `path`: URL path (e.g., "/" or "/index.html")
///
/// Example:
/// ```
/// let ip = dns::resolve("example.com").unwrap();
/// let response = http::get(ip, "example.com", "/");
/// ```
pub fn get(ip: [u8; 4], host: &str, path: &str) -> Result<HttpResponse, &'static str> {
    crate::serial_println!(
        "[HTTP] GET http://{}{}  ({})",
        host, path, Ipv4Packet::format_ip(&ip)
    );

    // Connect to port 80
    let mut conn = tcp::connect(ip, 80)?;

    // Build HTTP request
    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nUser-Agent: KafkaOS/1.0\r\nAccept: */*\r\n\r\n",
        path, host
    );

    crate::serial_println!("[HTTP] Sending request ({} bytes)...", request.len());

    // Send the request
    tcp::send(&mut conn, request.as_bytes())?;

    // Receive the response
    crate::serial_println!("[HTTP] Waiting for response...");

    let mut response_data = Vec::new();

    // Keep receiving until the connection closes or we timeout
    loop {
        match tcp::receive(&mut conn, 300) {
            Ok(data) => {
                if data.is_empty() {
                    break; // Server closed connection
                }
                response_data.extend_from_slice(&data);

                // If Connection: close, server will FIN when done
                if conn.remote_finished {
                    break;
                }
            }
            Err(_) => break,
        }
    }

    // Close our side
    let _ = tcp::close(&mut conn);

    if response_data.is_empty() {
        return Err("No response received");
    }

    crate::serial_println!("[HTTP] Received {} bytes total", response_data.len());

    // Parse the HTTP response
    parse_http_response(&response_data)
}

/// Parse raw HTTP response bytes into an HttpResponse.
fn parse_http_response(data: &[u8]) -> Result<HttpResponse, &'static str> {
    // Convert to string (lossy — handles non-UTF8 gracefully)
    let text = String::from_utf8_lossy(data);

    // Split headers from body at \r\n\r\n
    let (header_section, body) = match text.find("\r\n\r\n") {
        Some(pos) => (&text[..pos], text[pos + 4..].to_string()),
        None => {
            // Maybe just \n\n
            match text.find("\n\n") {
                Some(pos) => (&text[..pos], text[pos + 2..].to_string()),
                None => return Err("Could not find end of HTTP headers"),
            }
        }
    };

    // Parse status line: "HTTP/1.1 200 OK"
    let mut lines = header_section.lines();
    let status_line = lines.next().ok_or("Empty HTTP response")?;

    let mut parts = status_line.splitn(3, ' ');
    let _version = parts.next().ok_or("Missing HTTP version")?;
    let status_code: u16 = parts
        .next()
        .ok_or("Missing status code")?
        .parse()
        .map_err(|_| "Invalid status code")?;
    let status_text = parts.next().unwrap_or("").to_string();

    // Parse headers
    let mut headers = Vec::new();
    for line in lines {
        if let Some(colon_pos) = line.find(':') {
            let key = line[..colon_pos].trim().to_string();
            let value = line[colon_pos + 1..].trim().to_string();
            headers.push((key, value));
        }
    }

    Ok(HttpResponse {
        status_code,
        status_text: status_text.into(),
        headers,
        body,
    })
}