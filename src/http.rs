//! The little of HTTP/1.1 the server speaks: one request a connection, read
//! with caps on its head and its body, answered and closed; the replies,
//! each with the headers every answer carries; server-sent events, for a
//! job's progress and the chat's answer; and the checks that keep other
//! sites out. It's `std::net` and a thread a connection: the server is one
//! user's, on 127.0.0.1.
//!
//! A request whose `Host` isn't 127.0.0.1 or localhost is refused, which
//! keeps out a site that rebinds its name to this machine; and one that
//! does something (a POST, PUT or DELETE, or opening a file), from another
//! site's page by its `Origin` or `Sec-Fetch-Site`, is refused too.
//!
//! Adapted from crystal's `src/wiki_server.rs` (MIT).

use serde_json::{Value, json};
use std::borrow::Cow;
use std::io::{self, BufRead, Read, Write};

/// The longest a request's line and headers may be, and its body.
pub const MAX_HEAD: usize = 16 * 1024;
pub const MAX_BODY: usize = 64 * 1024;

/// A request, as far as the server reads it.
#[derive(Debug, Default)]
pub struct Request {
    pub method: String,
    /// The path, its escapes undone, without its query.
    pub path: String,
    pub query: String,
    /// The headers, their names in lower case.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        let found = self.headers.iter().find(|(header, _)| header == name);
        found.map(|(_, value)| value.as_str())
    }

    /// The value of `name` in the query, its escapes undone.
    pub fn param(&self, name: &str) -> Option<String> {
        self.query.split('&').find_map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            (decode(key, true)? == name).then(|| decode(value, true))?
        })
    }
}

/// Reads a request from `reader`, or the status that refuses it.
pub fn read_request(reader: &mut impl BufRead, out: &mut impl Write) -> Result<Request, u16> {
    let mut head = Vec::new();
    loop {
        let before = head.len();
        let read = reader
            .by_ref()
            .take((MAX_HEAD + 1 - head.len()) as u64)
            .read_until(b'\n', &mut head)
            .map_err(|_| 400u16)?;
        if read == 0 {
            return Err(400);
        }
        if head.len() > MAX_HEAD {
            return Err(431);
        }
        if head[before..].trim_ascii().is_empty() && before > 0 {
            break;
        }
    }
    let head = String::from_utf8(head).map_err(|_| 400u16)?;
    let mut lines = head.lines();
    let mut first = lines.next().unwrap_or_default().split(' ');
    let (method, target) = (
        first.next().unwrap_or_default(),
        first.next().unwrap_or_default(),
    );
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let mut request = Request {
        method: method.to_string(),
        path: decode(path, false).ok_or(400u16)?,
        query: query.to_string(),
        ..Request::default()
    };
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            request
                .headers
                .push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
        }
    }
    if request.header("transfer-encoding").is_some() {
        return Err(411);
    }
    let length: usize = match request.header("content-length") {
        Some(length) => length.parse().map_err(|_| 400u16)?,
        None => 0,
    };
    if length > MAX_BODY {
        return Err(413);
    }
    if length > 0 {
        if request
            .header("expect")
            .is_some_and(|expect| expect.eq_ignore_ascii_case("100-continue"))
        {
            let _ = out.write_all(b"HTTP/1.1 100 Continue\r\n\r\n");
        }
        request.body = vec![0; length];
        reader.read_exact(&mut request.body).map_err(|_| 400u16)?;
    }
    Ok(request)
}

/// `text` with its `%XX` escapes undone, and in a query its `+`s spaces;
/// `None` for an escape that isn't one, or what isn't UTF-8.
pub fn decode(text: &str, query: bool) -> Option<String> {
    let mut bytes = Vec::with_capacity(text.len());
    let mut rest = text.bytes();
    while let Some(byte) = rest.next() {
        match byte {
            b'%' => {
                let hex = [rest.next()?, rest.next()?];
                bytes.push(u8::from_str_radix(std::str::from_utf8(&hex).ok()?, 16).ok()?);
            }
            b'+' if query => bytes.push(b' '),
            byte => bytes.push(byte),
        }
    }
    String::from_utf8(bytes).ok()
}

/// Whether `host`, a `Host` header, names this machine as the server
/// listens on it: 127.0.0.1 or localhost, on any port.
pub fn is_local_host(host: Option<&str>) -> bool {
    let Some(host) = host else {
        return false;
    };
    let name = match host.rsplit_once(':') {
        Some((name, port)) if port.chars().all(|c| c.is_ascii_digit()) => name,
        _ => host,
    };
    name == "127.0.0.1" || name.eq_ignore_ascii_case("localhost")
}

/// Whether `origin` is the page's own: the server as `host`, the request's
/// `Host`, names it. That's this machine, checked already, on the port the
/// browser reached, which a container's published port may change.
fn is_own_origin(origin: &str, host: Option<&str>) -> bool {
    host.is_some_and(|host| is_local_host(Some(host)) && origin == format!("http://{host}"))
}

/// Why `request`, which does something, can't be taken from where it came:
/// another site's page, by its `Origin` or `Sec-Fetch-Site`; and a POST,
/// PUT or DELETE must say its origin, as browsers do.
pub fn foreign(request: &Request) -> Option<&'static str> {
    let changes = matches!(request.method.as_str(), "POST" | "PUT" | "DELETE");
    match request.header("origin") {
        Some(origin) if !is_own_origin(origin, request.header("host")) => {
            return Some("another site's page");
        }
        None if changes => return Some("a page that doesn't say its origin"),
        _ => {}
    }
    match request.header("sec-fetch-site") {
        Some("same-origin" | "none") | None => None,
        Some(_) => Some("another site's page"),
    }
}

/// An answer to a request, but for one that streams.
pub struct Reply {
    pub status: u16,
    content_type: &'static str,
    body: Cow<'static, [u8]>,
    headers: Vec<(&'static str, String)>,
}

impl Reply {
    pub fn bytes(content_type: &'static str, body: impl Into<Cow<'static, [u8]>>) -> Reply {
        Reply {
            status: 200,
            content_type,
            body: body.into(),
            headers: Vec::new(),
        }
    }

    pub fn json(value: &impl serde::Serialize) -> Reply {
        let body = serde_json::to_vec(value).unwrap_or_else(|_| b"null".to_vec());
        Reply::bytes("application/json", body)
    }

    /// An API's refusal: `status`, and `{"message": …}` saying why.
    pub fn error(status: u16, message: impl Into<String>) -> Reply {
        Reply {
            status,
            ..Reply::json(&json!({ "message": message.into() }))
        }
    }

    pub fn text(status: u16, text: impl Into<String>) -> Reply {
        Reply {
            status,
            ..Reply::bytes("text/plain; charset=utf-8", text.into().into_bytes())
        }
    }

    pub fn empty() -> Reply {
        Reply {
            status: 204,
            ..Reply::bytes("text/plain", Vec::new())
        }
    }

    pub fn with(mut self, name: &'static str, value: &str) -> Reply {
        self.headers.push((name, value.to_string()));
        self
    }

    pub fn write(&self, out: &mut impl Write) -> io::Result<()> {
        let mut head = format!("HTTP/1.1 {} {}\r\n", self.status, reason(self.status));
        if self.status != 204 {
            head.push_str(&format!(
                "Content-Type: {}\r\nContent-Length: {}\r\n",
                self.content_type,
                self.body.len()
            ));
        }
        if !self
            .headers
            .iter()
            .any(|(name, _)| *name == "Cache-Control")
        {
            head.push_str("Cache-Control: no-cache\r\n");
        }
        for (name, value) in &self.headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        head.push_str(SECURITY_HEADERS);
        out.write_all(head.as_bytes())?;
        out.write_all(&self.body)?;
        out.flush()
    }
}

/// What every answer says: never sniffed for another type, never framed by
/// another site, never telling a link's site where it was followed from,
/// and the connection closed after it.
const SECURITY_HEADERS: &str = "X-Content-Type-Options: nosniff\r\nX-Frame-Options: DENY\r\n\
     Referrer-Policy: no-referrer\r\nConnection: close\r\n\r\n";

/// Starts an answer of server-sent events, written one by one with
/// [`event`] after it.
pub fn start_events(out: &mut impl Write) -> io::Result<()> {
    out.write_all(
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\
             Cache-Control: no-store\r\n{SECURITY_HEADERS}"
        )
        .as_bytes(),
    )?;
    out.flush()
}

/// Writes the event `name` with `data`, and sends it on.
pub fn event(out: &mut (impl Write + ?Sized), name: &str, data: &Value) -> io::Result<()> {
    write!(out, "event: {name}\ndata: {data}\n\n")?;
    out.flush()
}

/// Writes a comment, which a page passes over: how a stream that has
/// nothing to say for a while finds out whether its page is still there.
pub fn keep_alive(out: &mut (impl Write + ?Sized)) -> io::Result<()> {
    out.write_all(b": still here\n\n")?;
    out.flush()
}

pub fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        411 => "Length Required",
        413 => "Content Too Large",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "Unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(raw: &str) -> Result<Request, u16> {
        read_request(&mut raw.as_bytes(), &mut Vec::new())
    }

    #[test]
    fn a_request_is_read_with_its_headers_query_and_body() {
        let request = read(
            "POST /api/repos/app/ask?x=1 HTTP/1.1\r\nHost: 127.0.0.1:7347\r\n\
             Content-Length: 5\r\nOrigin: http://127.0.0.1:7347\r\n\r\nhello",
        )
        .unwrap();
        assert_eq!(request.method, "POST");
        assert_eq!(request.path, "/api/repos/app/ask");
        assert_eq!(request.query, "x=1");
        assert_eq!(request.header("host"), Some("127.0.0.1:7347"));
        assert_eq!(request.header("origin"), Some("http://127.0.0.1:7347"));
        assert_eq!(request.body, b"hello");
    }

    #[test]
    fn a_request_too_big_or_chunked_is_refused() {
        let long = format!("GET / HTTP/1.1\r\nX: {}\r\n\r\n", "a".repeat(MAX_HEAD));
        assert_eq!(read(&long).unwrap_err(), 431);
        let big = format!(
            "POST / HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
            MAX_BODY + 1
        );
        assert_eq!(read(&big).unwrap_err(), 413);
        let chunked = "POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n";
        assert_eq!(read(chunked).unwrap_err(), 411);
        assert_eq!(read("GET / HTTP/1.1\r\n").unwrap_err(), 400, "cut short");
        assert_eq!(read("GET /%zz HTTP/1.1\r\n\r\n").unwrap_err(), 400);
    }

    #[test]
    fn a_query_s_escapes_are_undone() {
        let request = read("GET /api/repos/a/open?path=src%2Fmy+file.rs&line=12 HTTP/1.1\r\n\r\n");
        let request = request.unwrap();
        assert_eq!(request.param("path").as_deref(), Some("src/my file.rs"));
        assert_eq!(request.param("line").as_deref(), Some("12"));
        assert_eq!(request.param("end"), None);
    }

    #[test]
    fn only_this_machine_s_names_are_answered() {
        assert!(is_local_host(Some("127.0.0.1:7347")));
        assert!(is_local_host(Some("localhost:7347")));
        assert!(is_local_host(Some("LOCALHOST")));
        assert!(!is_local_host(Some("evil.example:7347")));
        assert!(!is_local_host(Some("127.0.0.1.evil.example")));
        assert!(!is_local_host(None));
    }

    #[test]
    fn what_does_something_must_come_from_the_server_s_own_page() {
        let request = |method: &str, headers: &[(&str, &str)]| Request {
            method: method.to_string(),
            headers: (headers.iter())
                .map(|(name, value)| (name.to_string(), value.to_string()))
                .collect(),
            ..Request::default()
        };
        let host = ("host", "127.0.0.1:7347");
        let own = ("origin", "http://127.0.0.1:7347");
        for method in ["POST", "PUT", "DELETE"] {
            assert_eq!(foreign(&request(method, &[host, own])), None, "{method}");
            assert!(foreign(&request(method, &[host])).is_some(), "{method}");
        }
        let localhost = [
            ("host", "localhost:7347"),
            ("origin", "http://localhost:7347"),
        ];
        assert_eq!(foreign(&request("POST", &localhost)), None);
        // A container's port, published as another.
        let published = [
            ("host", "127.0.0.1:8080"),
            ("origin", "http://127.0.0.1:8080"),
        ];
        assert_eq!(foreign(&request("POST", &published)), None);
        let elsewhere = [("host", "127.0.0.1:8000"), own];
        assert!(
            foreign(&request("POST", &elsewhere)).is_some(),
            "another port"
        );
        let rebound = [("host", "evil.example"), ("origin", "http://evil.example")];
        assert!(foreign(&request("POST", &rebound)).is_some());
        let evil = ("origin", "https://evil.example");
        assert!(foreign(&request("POST", &[host, evil])).is_some());
        assert_eq!(foreign(&request("GET", &[host])), None, "typed in");
        let fetched = request("GET", &[host, ("sec-fetch-site", "same-origin")]);
        assert_eq!(foreign(&fetched), None);
        let embedded = request("GET", &[host, ("sec-fetch-site", "cross-site")]);
        assert!(foreign(&embedded).is_some());
    }

    #[test]
    fn a_reply_says_its_length_type_and_that_it_closes() {
        let mut out = Vec::new();
        Reply::text(404, "gone").write(&mut out).unwrap();
        let out = String::from_utf8(out).unwrap();
        assert!(out.starts_with("HTTP/1.1 404 Not Found\r\n"), "{out}");
        assert!(out.contains("Content-Type: text/plain; charset=utf-8\r\n"));
        assert!(out.contains("Content-Length: 4\r\n"));
        assert!(out.contains("X-Content-Type-Options: nosniff\r\n"));
        assert!(out.ends_with("Connection: close\r\n\r\ngone"));
        let mut out = Vec::new();
        Reply::empty().write(&mut out).unwrap();
        assert!(!String::from_utf8(out).unwrap().contains("Content-Length"));
        let mut out = Vec::new();
        Reply::error(409, "busy").write(&mut out).unwrap();
        let out = String::from_utf8(out).unwrap();
        assert!(out.starts_with("HTTP/1.1 409 Conflict\r\n"));
        assert!(out.ends_with("{\"message\":\"busy\"}"));
    }

    #[test]
    fn events_are_written_as_the_page_reads_them() {
        let mut out = Vec::new();
        event(&mut out, "delta", &json!({"text": "a \"b\"\nc"})).unwrap();
        keep_alive(&mut out).unwrap();
        event(&mut out, "done", &json!({"version": 3})).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "event: delta\ndata: {\"text\":\"a \\\"b\\\"\\nc\"}\n\n: still here\n\n\
             event: done\ndata: {\"version\":3}\n\n"
        );
    }
}
