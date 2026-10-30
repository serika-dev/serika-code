use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

pub struct Request {
    pub method: String,
    pub path: String,
    pub body: String,
}

pub struct Response {
    pub status: u16,
    pub content_type: &'static str,
    pub body: String,
}

pub async fn spawn(
    handler: impl Fn(Request) -> Response + Send + Sync + 'static,
) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let handler = Arc::new(handler);
    let task = tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                break;
            };
            let handler = handler.clone();
            tokio::spawn(async move {
                if let Ok(request) = read_request(&mut socket).await {
                    let response = handler(request);
                    let reason = match response.status {
                        200 => "OK",
                        401 => "Unauthorized",
                        429 => "Too Many Requests",
                        _ => "Error",
                    };
                    let header = format!(
                        "HTTP/1.1 {} {reason}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        response.status,
                        response.content_type,
                        response.body.len()
                    );
                    let _ = socket.write_all(header.as_bytes()).await;
                    let _ = socket.write_all(response.body.as_bytes()).await;
                }
            });
        }
    });
    (format!("http://{addr}"), task)
}

async fn read_request(socket: &mut tokio::net::TcpStream) -> std::io::Result<Request> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 8192];
    let header_end = loop {
        let n = socket.read(&mut tmp).await?;
        if n == 0 {
            break None;
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(index) = buf.windows(4).position(|window| window == b"\r\n\r\n") {
            break Some(index + 4);
        }
        if buf.len() > 2_000_000 {
            break None;
        }
    };
    let Some(header_end) = header_end else {
        return Err(std::io::Error::other("incomplete request"));
    };
    let header = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let mut content_length = 0usize;
    for line in header.lines() {
        if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            content_length = value.trim().parse().unwrap_or(0);
        }
    }
    while buf.len() < header_end + content_length {
        let n = socket.read(&mut tmp).await?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
    }
    let body = String::from_utf8_lossy(&buf[header_end..header_end + content_length.min(buf.len() - header_end)]).to_string();
    let request_line = header.lines().next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("").to_string();
    Ok(Request { method, path, body })
}
