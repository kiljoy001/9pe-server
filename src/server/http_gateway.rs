use anyhow::{Result, Context};
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::path::Path;
use tracing::{info, error, debug};
use crate::traits::StorageProvider;

pub struct HttpGateway {
    storage: Arc<dyn StorageProvider>,
}

impl HttpGateway {
    pub fn new(storage: Arc<dyn StorageProvider>) -> Self {
        Self { storage }
    }

    pub async fn run(&self, port: u16) -> Result<()> {
        let listener = TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
        info!("HTTP Gateway listening on http://localhost:{}", port);

        loop {
            let (mut socket, _) = listener.accept().await?;
            let storage = self.storage.clone();

            tokio::spawn(async move {
                if let Err(e) = handle_http_request(&mut socket, storage).await {
                    debug!("HTTP Gateway request error: {}", e);
                }
            });
        }
    }
}

async fn handle_http_request(socket: &mut TcpStream, storage: Arc<dyn StorageProvider>) -> Result<()> {
    let mut buffer = [0u8; 4096];
    let n = socket.read(&mut buffer).await?;
    let request = String::from_utf8_lossy(&buffer[..n]);

    let lines: Vec<&str> = request.lines().collect();
    if lines.is_empty() { return Ok(()); }

    let request_line = lines[0];
    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 { return Ok(()); }

    let method = parts[0];
    let path_str = parts[1];

    debug!("HTTP Gateway: {} {}", method, path_str);

    // Simple routing: mapping URLs to 9P paths
    // e.g. /n/web/foo -> storage.read(/n/web/foo)
    
    // Serve landing page at root
    if path_str == "/" {
        let html = include_str!("remote_dom_landing.html");
        send_response(socket, "text/html", html.as_bytes()).await?;
        return Ok(());
    }

    // Map /n/ to 9P namespace
    if path_str.starts_with("/n/") {
        // Strip query parameters for file path and MIME detection
        let path_without_query = path_str.split('?').next().unwrap_or(path_str);
        let p = Path::new(&path_without_query[1..]); // Remove leading / -> n/web/...

        if method == "GET" {
            match storage.read(p, 0, 10 * 1024 * 1024).await { // 10MB limit for images
                Ok(data) => {
                    let mime = if path_without_query.ends_with(".png") { "image/png" }
                               else if path_without_query.ends_with(".html") { "text/html" }
                               else if path_without_query.ends_with(".json") { "application/json" }
                               else if path_without_query.ends_with(".css") { "text/css" }
                               else if path_without_query.ends_with(".js") { "application/javascript" }
                               else { "application/octet-stream" };
                    send_response(socket, mime, &data).await?;
                }
                Err(e) => {
                    send_error(socket, 404, &format!("Not Found: {}", e)).await?;
                }
            }
            return Ok(());
        } else if method == "POST" {
            // Find body start
            if let Some(body_start) = request.find("\r\n\r\n") {
                let data = request[body_start+4..].as_bytes();
                match storage.write(p, 0, data).await {
                    Ok(written) => {
                        let resp = format!(r#"{{"written": {}}}"#, written);
                        send_response(socket, "application/json", resp.as_bytes()).await?;
                    }
                    Err(e) => {
                        send_error(socket, 500, &format!("Write error: {}", e)).await?;
                    }
                }
            }
            return Ok(());
        }
    }

    send_error(socket, 404, "Not Found").await?;
    Ok(())
}

async fn send_response(socket: &mut TcpStream, mime: &str, body: &[u8]) -> Result<()> {
    let response = format!(
        "HTTP/1.1 200 OK\r\n\
         Content-Type: {}\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         Access-Control-Allow-Origin: *\r\n\
         \r\n",
        mime, body.len()
    );
    socket.write_all(response.as_bytes()).await?;
    socket.write_all(body).await?;
    Ok(())
}

async fn send_error(socket: &mut TcpStream, code: u16, message: &str) -> Result<()> {
    let status = match code {
        404 => "404 Not Found",
        _ => "500 Internal Server Error",
    };
    let body = format!("<html><body><h1>{}</h1><p>{}</p></body></html>", status, message);
    let response = format!(
        "HTTP/1.1 {}\r\n\
         Content-Type: text/html\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n",
        status, body.len()
    );
    socket.write_all(response.as_bytes()).await?;
    socket.write_all(body.as_bytes()).await?;
    Ok(())
}
