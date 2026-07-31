//! Socket-level integration tests for the API client: a minimal HTTP/1.1
//! responder on 127.0.0.1 plays the Tragentics API with canned responses in
//! the platform's exact shapes. No external network, no real platform.

use heartbeat_control_center_lib::api::{is_valid_token_format, ApiClient};
use heartbeat_control_center_lib::errors::ApiError;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

/// Serve `count` connections, each answered with `response` after reading the
/// request head. Returns the bound base URL.
fn serve(count: usize, response: String) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        for _ in 0..count {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let response = response.clone();
            thread::spawn(move || {
                let mut buf = [0u8; 8192];
                let mut read_total = 0usize;
                // Read until end of headers (requests here are small).
                loop {
                    match stream.read(&mut buf[read_total..]) {
                        Ok(0) => break,
                        Ok(n) => {
                            read_total += n;
                            let head = &buf[..read_total];
                            if windows_contains(head, b"\r\n\r\n") {
                                // If a body is declared, read what's advertised.
                                let head_str = String::from_utf8_lossy(head);
                                let content_len = head_str
                                    .lines()
                                    .find_map(|l| {
                                        let l = l.to_ascii_lowercase();
                                        l.strip_prefix("content-length:")
                                            .map(|v| v.trim().parse::<usize>().ok())
                                    })
                                    .flatten()
                                    .unwrap_or(0);
                                let body_have = read_total
                                    - (head_str
                                        .find("\r\n\r\n")
                                        .map(|i| i + 4)
                                        .unwrap_or(read_total));
                                if body_have >= content_len {
                                    break;
                                }
                            }
                            if read_total == buf.len() {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            });
        }
    });
    format!("http://127.0.0.1:{}", addr.port())
}

fn windows_contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

fn http_response(status: u16, reason: &str, extra_headers: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n{extra_headers}\r\n{body}",
        body.len()
    )
}

fn demo_token() -> String {
    format!("tk_{}", "0123456789abcdef".repeat(4))
}

#[tokio::test]
async fn verify_token_parses_me_response() {
    let body = r#"{"id":"11111111-2222-4333-8444-555555555555","name":"Socket Agent","slug":"socket-agent","status":"offline","is_public":false,"is_revoked":false,"is_archived":false,"last_heartbeat":null,"created_at":"2026-07-31T00:00:00.000Z"}"#;
    let base = serve(1, http_response(200, "OK", "", body));
    let client = ApiClient::new();
    let me = client
        .verify_token(&base, &demo_token())
        .await
        .expect("verify ok");
    assert_eq!(me.name, "Socket Agent");
    assert_eq!(me.status, "offline");
    assert!(me.last_heartbeat.is_none());
}

#[tokio::test]
async fn heartbeat_success_parses_agent_state() {
    let body = r#"{"heartbeat":"accepted","agent":{"id":"a1","status":"online","last_heartbeat":"2026-07-31T01:02:03.000Z"}}"#;
    let base = serve(1, http_response(200, "OK", "", body));
    let client = ApiClient::new();
    let result = client
        .send_heartbeat(&base, &demo_token(), "a1", "online")
        .await
        .expect("beat ok");
    assert_eq!(result.platform_status, "online");
    assert_eq!(
        result.platform_last_heartbeat.as_deref(),
        Some("2026-07-31T01:02:03.000Z")
    );
}

#[tokio::test]
async fn unauthorized_maps_with_platform_message() {
    let base = serve(
        1,
        http_response(
            401,
            "Unauthorized",
            "",
            r#"{"error":"Your agent has been revoked"}"#,
        ),
    );
    let client = ApiClient::new();
    let err = client
        .send_heartbeat(&base, &demo_token(), "a1", "online")
        .await
        .expect_err("must fail");
    match err {
        ApiError::Unauthorized { message } => assert_eq!(message, "Your agent has been revoked"),
        other => panic!("expected unauthorized, got {other:?}"),
    }
}

#[tokio::test]
async fn rate_limit_maps_retry_after_header() {
    let base = serve(
        1,
        http_response(
            429,
            "Too Many Requests",
            "retry-after: 42\r\n",
            r#"{"error":"Too many requests"}"#,
        ),
    );
    let client = ApiClient::new();
    let err = client
        .send_heartbeat(&base, &demo_token(), "a1", "online")
        .await
        .expect_err("must fail");
    match err {
        ApiError::RateLimited { retry_after_secs } => assert_eq!(retry_after_secs, 42),
        other => panic!("expected rate-limited, got {other:?}"),
    }
}

#[tokio::test]
async fn connection_refused_is_network_error() {
    // Bind + drop a listener to get a port that refuses connections.
    let port = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let client = ApiClient::new();
    let err = client
        .send_heartbeat(
            &format!("http://127.0.0.1:{port}"),
            &demo_token(),
            "a1",
            "online",
        )
        .await
        .expect_err("must fail");
    assert!(matches!(err, ApiError::Network { .. }), "got {err:?}");
}

#[tokio::test]
async fn garbage_body_is_decode_error() {
    let base = serve(1, http_response(200, "OK", "", r#"{"unexpected":"shape"}"#));
    let client = ApiClient::new();
    let err = client
        .send_heartbeat(&base, &demo_token(), "a1", "online")
        .await
        .expect_err("must fail");
    assert!(matches!(err, ApiError::Decode { .. }), "got {err:?}");
}

#[test]
fn mock_tokens_match_platform_format() {
    assert!(is_valid_token_format(&demo_token()));
}
