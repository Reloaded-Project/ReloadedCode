use std::io::{BufRead, Write};

/// A response for the mock HTTP server to serve to the fetch request.
///
/// Variants exercise the different `fetch_catalog` outcomes:
/// - `Ok` - a full 200 response with an ETag and body
/// - `PartialOk` - a 200 whose declared `Content-Length` differs from the body
/// - `NotModified` - a 304 echoing the provided ETag
/// - `Status` - an arbitrary status code with a reason phrase
pub enum MockResponse {
    Ok {
        etag: &'static str,
        body: String,
    },
    PartialOk {
        etag: &'static str,
        body: String,
        content_length: usize,
    },
    NotModified {
        etag: &'static str,
    },
    Status {
        code: u16,
        reason: &'static str,
    },
}

/// Sample `api.json` payload bytes matching the models.dev schema, used as the
/// body for mock catalog responses.
pub fn sample_api_json() -> &'static [u8] {
    br#"
        {
            "openai": {
                "id": "openai",
                "npm": "@ai-sdk/openai",
                "api": "https://api.openai.com/v1",
                "env": ["OPENAI_API_KEY"],
                "models": {
                    "gpt-4": {
                        "modalities": {
                            "input": ["text"],
                            "output": ["text"]
                        },
                        "limit": {
                            "context": 8192,
                            "input": 8192,
                            "output": 4096
                        }
                    }
                }
            }
        }
        "#
}

/// Starts a threaded mock HTTP server that serves a single request for `/api.json`.
///
/// Returns the server thread's join handle and the base URL to fetch from.
///
/// # Arguments
///
/// - `response` - the [`MockResponse`] the server should send.
pub fn start_mock_server(response: MockResponse) -> (std::thread::JoinHandle<()>, String) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    let url = format!("http://127.0.0.1:{}/api.json", port);

    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let mut reader = std::io::BufReader::new(&stream);
        let mut request = String::new();

        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).expect("read line") == 0 {
                break;
            }
            if line == "\r\n" || line.is_empty() {
                break;
            }
            request.push_str(&line);
        }

        let _has_if_none_match = request.contains("If-None-Match");

        match response {
            MockResponse::Ok { etag, body } => {
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nETag: {}\r\nContent-Length: {}\r\n\r\n{}",
                    etag,
                    body.len(),
                    body
                );
                stream.write_all(response.as_bytes()).expect("write");
                stream.flush().expect("flush");
            }
            MockResponse::PartialOk {
                etag,
                body,
                content_length,
            } => {
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nETag: {}\r\nContent-Length: {}\r\n\r\n{}",
                    etag, content_length, body
                );
                stream.write_all(response.as_bytes()).expect("write");
                stream.flush().expect("flush");
            }
            MockResponse::NotModified { etag } => {
                let response = format!("HTTP/1.1 304 Not Modified\r\nETag: {}\r\n\r\n", etag);
                stream.write_all(response.as_bytes()).expect("write");
                stream.flush().expect("flush");
            }
            MockResponse::Status { code, reason } => {
                let response = format!("HTTP/1.1 {code} {reason}\r\nContent-Length: 0\r\n\r\n");
                stream.write_all(response.as_bytes()).expect("write");
                stream.flush().expect("flush");
            }
        }
    });

    (handle, url)
}
