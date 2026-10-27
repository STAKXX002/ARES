//! A minimal HTTP/1.1 client for talking to a server on this machine.
//!
//! It exists for one purpose: calling a local model server (Ollama) without
//! adding a dependency. It is deliberately narrow:
//!
//! - **Loopback only.** Any address that is not a loopback address is refused
//!   before a connection is attempted, so prompts cannot be sent off the
//!   machine by a configuration mistake.
//! - Plain HTTP, `POST` with a JSON body, `Connection: close`.
//! - Every phase is bounded: connect, the whole exchange (one deadline), and
//!   the size of the response.
//! - Handles `Content-Length`, `Transfer-Encoding: chunked`, and
//!   read-until-close responses.

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}

#[derive(Debug)]
pub enum HttpError {
    /// The address is not a loopback address. Nothing was sent.
    NotLoopback,
    /// The request path or host was not acceptable. Nothing was sent.
    InvalidRequest(String),
    /// No server answered at the address.
    Connect(io::Error),
    Io(io::Error),
    /// The exchange did not finish before the deadline.
    Timeout,
    /// The response was larger than the limit.
    TooLarge,
    /// The response was not valid HTTP.
    Malformed(String),
}

const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const HEADER_LIMIT: usize = 64 * 1024;

fn valid_path(path: &str) -> bool {
    path.starts_with('/')
        && path.len() <= 200
        && path.bytes().all(|b| b.is_ascii_graphic() && b != b'?' && b != b'#')
}

/// Send `body` (JSON) to `addr` at `path` and read the whole response.
///
/// `deadline` covers sending and receiving. `max_response` bounds the total
/// bytes read, headers included.
pub fn post_json(
    addr: SocketAddr,
    path: &str,
    body: &str,
    deadline: Duration,
    max_response: usize,
) -> Result<Response, HttpError> {
    if !addr.ip().is_loopback() {
        return Err(HttpError::NotLoopback);
    }
    if !valid_path(path) {
        return Err(HttpError::InvalidRequest(format!("path {path:?} is not allowed")));
    }

    let start = Instant::now();
    let mut stream = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT.min(deadline))
        .map_err(HttpError::Connect)?;

    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {addr}\r\nUser-Agent: ares\r\nContent-Type: application/json\r\nAccept: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );

    let remaining = |start: Instant| deadline.checked_sub(start.elapsed()).filter(|d| !d.is_zero());

    stream
        .set_write_timeout(remaining(start))
        .map_err(HttpError::Io)?;
    for part in [request.as_bytes(), body.as_bytes()] {
        stream.write_all(part).map_err(|e| match e.kind() {
            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut => HttpError::Timeout,
            _ => HttpError::Io(e),
        })?;
    }
    stream.flush().map_err(HttpError::Io)?;

    let mut raw: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let left = remaining(start).ok_or(HttpError::Timeout)?;
        stream.set_read_timeout(Some(left)).map_err(HttpError::Io)?;
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                if raw.len() + n > max_response {
                    return Err(HttpError::TooLarge);
                }
                raw.extend_from_slice(&chunk[..n]);
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {
                return Err(HttpError::Timeout)
            }
            // The server may reset the connection after sending everything.
            Err(e) if e.kind() == io::ErrorKind::ConnectionReset && !raw.is_empty() => break,
            Err(e) => return Err(HttpError::Io(e)),
        }
    }
    parse_response(&raw, max_response)
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn malformed(msg: &str) -> HttpError {
    HttpError::Malformed(msg.to_string())
}

fn parse_response(raw: &[u8], max_body: usize) -> Result<Response, HttpError> {
    let head_end = find(raw, b"\r\n\r\n").ok_or_else(|| malformed("no end of headers"))?;
    if head_end > HEADER_LIMIT {
        return Err(malformed("headers too large"));
    }
    let head = std::str::from_utf8(&raw[..head_end]).map_err(|_| malformed("headers are not text"))?;
    let rest = &raw[head_end + 4..];

    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap_or("");
    let mut parts = status_line.splitn(3, ' ');
    let version = parts.next().unwrap_or("");
    if !version.starts_with("HTTP/1.") {
        return Err(malformed("not an HTTP/1.x response"));
    }
    let status: u16 = parts
        .next()
        .and_then(|s| s.parse().ok())
        .filter(|s| (100..=599).contains(s))
        .ok_or_else(|| malformed("bad status code"))?;

    let mut content_length: Option<usize> = None;
    let mut chunked = false;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            let value = value.trim();
            match name.trim().to_ascii_lowercase().as_str() {
                "content-length" => {
                    content_length = Some(value.parse().map_err(|_| malformed("bad content-length"))?)
                }
                "transfer-encoding" => {
                    chunked = value.to_ascii_lowercase().contains("chunked");
                }
                _ => {}
            }
        }
    }

    let body = if chunked {
        decode_chunked(rest, max_body)?
    } else if let Some(n) = content_length {
        if n > max_body {
            return Err(HttpError::TooLarge);
        }
        if rest.len() < n {
            return Err(malformed("body is shorter than content-length"));
        }
        rest[..n].to_vec()
    } else {
        rest.to_vec()
    };
    Ok(Response { status, body })
}

fn decode_chunked(mut data: &[u8], max_body: usize) -> Result<Vec<u8>, HttpError> {
    let mut out = Vec::new();
    loop {
        let line_end = find(data, b"\r\n").ok_or_else(|| malformed("truncated chunk header"))?;
        let size_text = std::str::from_utf8(&data[..line_end]).map_err(|_| malformed("bad chunk header"))?;
        // Anything after ';' is a chunk extension, which is ignored.
        let size_text = size_text.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_text, 16).map_err(|_| malformed("bad chunk size"))?;
        data = &data[line_end + 2..];
        if size == 0 {
            return Ok(out);
        }
        if out.len() + size > max_body {
            return Err(HttpError::TooLarge);
        }
        if data.len() < size + 2 {
            return Err(malformed("truncated chunk"));
        }
        out.extend_from_slice(&data[..size]);
        if &data[size..size + 2] != b"\r\n" {
            return Err(malformed("chunk is not followed by CRLF"));
        }
        data = &data[size + 2..];
    }
}
