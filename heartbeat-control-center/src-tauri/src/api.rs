use crate::errors::ApiError;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

pub const USER_AGENT: &str = concat!(
    "TragenticsHeartbeatControlCenter/",
    env!("CARGO_PKG_VERSION"),
    " (+https://tragentics.com)"
);

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

/// Shape of GET /api/agents/me — the platform returns the agent object bare
/// (app/api/agents/me/route.ts → ok(agent)).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeAgent {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub slug: Option<String>,
    pub status: String,
    #[serde(default)]
    pub is_public: bool,
    #[serde(default)]
    pub is_revoked: bool,
    #[serde(default)]
    pub is_archived: bool,
    #[serde(default)]
    pub last_heartbeat: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
}

/// Shape of POST /api/agents/{id}/heartbeat 200 —
/// ok({ heartbeat: 'accepted', agent: { id, status, last_heartbeat } }).
#[derive(Debug, Clone, Deserialize)]
pub struct HeartbeatResponse {
    #[allow(dead_code)]
    pub heartbeat: String,
    pub agent: HeartbeatAgent,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HeartbeatAgent {
    #[allow(dead_code)]
    pub id: String,
    pub status: String,
    #[serde(default)]
    pub last_heartbeat: Option<String>,
}

#[derive(Debug, Clone)]
pub struct BeatSuccess {
    pub platform_status: String,
    pub platform_last_heartbeat: Option<String>,
    pub latency_ms: u32,
}

#[derive(Debug, Clone, Deserialize)]
struct ErrorBody {
    error: Option<String>,
}

pub struct ApiClient {
    http: reqwest::Client,
}

impl Default for ApiClient {
    fn default() -> Self {
        Self::new()
    }
}

impl ApiClient {
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .expect("reqwest client");
        Self { http }
    }

    /// GET {base}/api/agents/me with the agent token. Resolves the agent's
    /// identity from the token alone — used at add time and for reconciliation.
    pub async fn verify_token(&self, base_url: &str, token: &str) -> Result<MeAgent, ApiError> {
        let url = format!("{}/api/agents/me", base_url.trim_end_matches('/'));
        let resp = self
            .http
            .get(url)
            .bearer_auth(token)
            .send()
            .await
            .map_err(map_reqwest_error)?;
        let status = resp.status().as_u16();
        if status == 200 {
            let body = resp.bytes().await.map_err(map_reqwest_error)?;
            return serde_json::from_slice::<MeAgent>(&body).map_err(|e| ApiError::Decode {
                message: format!("unexpected /me response: {e}"),
            });
        }
        Err(map_error_response(status, resp).await)
    }

    /// POST {base}/api/agents/{agent_id}/heartbeat with {"status": "online"|"offline"}.
    pub async fn send_heartbeat(
        &self,
        base_url: &str,
        token: &str,
        agent_id: &str,
        status: &str,
    ) -> Result<BeatSuccess, ApiError> {
        let url = format!(
            "{}/api/agents/{}/heartbeat",
            base_url.trim_end_matches('/'),
            agent_id
        );
        let started = Instant::now();
        let resp = self
            .http
            .post(url)
            .bearer_auth(token)
            .json(&serde_json::json!({ "status": status }))
            .send()
            .await
            .map_err(map_reqwest_error)?;
        let latency_ms = started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32;
        let http_status = resp.status().as_u16();
        if http_status == 200 {
            let body = resp.bytes().await.map_err(map_reqwest_error)?;
            let parsed = serde_json::from_slice::<HeartbeatResponse>(&body).map_err(|e| {
                ApiError::Decode {
                    message: format!("unexpected heartbeat response: {e}"),
                }
            })?;
            return Ok(BeatSuccess {
                platform_status: parsed.agent.status,
                platform_last_heartbeat: parsed.agent.last_heartbeat,
                latency_ms,
            });
        }
        Err(map_error_response(http_status, resp).await)
    }

    /// Reachability probe for a base URL: GET /api/agents/me WITHOUT auth.
    /// The platform answers 401 {"error": "..."} when reachable — that IS the
    /// success signal here. Never sends any credential.
    pub async fn test_base_url(&self, base_url: &str) -> Result<String, ApiError> {
        let url = format!("{}/api/agents/me", base_url.trim_end_matches('/'));
        let resp = self.http.get(url).send().await.map_err(map_reqwest_error)?;
        let status = resp.status().as_u16();
        match status {
            401 => Ok("Reachable — Tragentics API answered as expected.".to_string()),
            200..=399 => Ok(format!(
                "Reachable (HTTP {status}) — but this does not look like the Tragentics API."
            )),
            _ => Ok(format!(
                "Host answered HTTP {status} — this does not look like the Tragentics API."
            )),
        }
    }

    /// Local health check for self-hosted agents: plain GET, pass when the
    /// status code falls inside [expect_min, expect_max].
    pub async fn local_check(
        &self,
        url: &str,
        expect_min: u16,
        expect_max: u16,
        timeout_secs: u64,
    ) -> Result<u16, String> {
        let resp = self
            .http
            .get(url)
            .timeout(Duration::from_secs(timeout_secs.clamp(1, 30)))
            .send()
            .await
            .map_err(|e| network_message(&e))?;
        let status = resp.status().as_u16();
        if status >= expect_min && status <= expect_max {
            Ok(status)
        } else {
            Err(format!(
                "HTTP {status} outside expected {expect_min}–{expect_max}"
            ))
        }
    }
}

fn network_message(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        "request timed out".to_string()
    } else if e.is_connect() {
        "could not connect".to_string()
    } else {
        // reqwest errors chain the useful cause; keep it short.
        let mut msg = e.to_string();
        if let Some(src) = std::error::Error::source(e) {
            msg = format!("{src}");
        }
        msg.chars().take(160).collect()
    }
}

fn map_reqwest_error(e: reqwest::Error) -> ApiError {
    ApiError::Network {
        message: network_message(&e),
    }
}

async fn map_error_response(status: u16, resp: reqwest::Response) -> ApiError {
    let retry_after = resp
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    let message = match resp.bytes().await {
        Ok(bytes) => serde_json::from_slice::<ErrorBody>(&bytes)
            .ok()
            .and_then(|b| b.error)
            .unwrap_or_else(|| format!("HTTP {status}")),
        Err(_) => format!("HTTP {status}"),
    };
    map_status(status, message, retry_after)
}

/// Pure status→error mapping (unit-tested without sockets).
pub fn map_status(status: u16, message: String, retry_after: Option<u64>) -> ApiError {
    match status {
        401 => ApiError::Unauthorized { message },
        403 => ApiError::Forbidden { message },
        404 => ApiError::NotFound { message },
        409 => ApiError::Conflict { message },
        429 => ApiError::RateLimited {
            retry_after_secs: retry_after.unwrap_or(60),
        },
        500..=599 => ApiError::Server { status, message },
        _ => ApiError::Other { status, message },
    }
}

/// Agent tokens are `tk_` + 64 lowercase hex chars (lib/utils/format.ts
/// generateApiKey). Validate before any storage or network use.
pub fn is_valid_token_format(token: &str) -> bool {
    let Some(hex) = token.strip_prefix("tk_") else {
        return false;
    };
    hex.len() == 64
        && hex
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// tk_ab12…89ef — enough to recognize, never enough to use.
pub fn token_fingerprint(token: &str) -> String {
    if token.len() < 12 {
        return "tk_…".to_string();
    }
    format!("{}…{}", &token[..7], &token[token.len() - 4..])
}

/// Base URL sanity: http(s), no trailing slash, https required except localhost.
pub fn normalize_base_url(input: &str) -> Result<String, String> {
    let trimmed = input.trim().trim_end_matches('/').to_string();
    if trimmed.is_empty() {
        return Err("Base URL cannot be empty".into());
    }
    let lower = trimmed.to_ascii_lowercase();
    let host_part = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"));
    let Some(host) = host_part else {
        return Err("Base URL must start with https:// (or http:// for localhost)".into());
    };
    if lower.starts_with("http://") {
        let is_local = host.starts_with("localhost")
            || host.starts_with("127.0.0.1")
            || host.starts_with("[::1]");
        if !is_local {
            return Err("Plain http:// is only allowed for localhost".into());
        }
    }
    if host.is_empty() {
        return Err("Base URL has no host".into());
    }
    Ok(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_format_accepts_platform_shape() {
        let good = format!("tk_{}", "0123456789abcdef".repeat(4));
        assert_eq!(good.len(), 67);
        assert!(is_valid_token_format(&good));
    }

    #[test]
    fn token_format_rejects_bad_shapes() {
        assert!(!is_valid_token_format(""));
        assert!(!is_valid_token_format("tk_"));
        assert!(!is_valid_token_format("sk_0123"));
        // uppercase hex is not what generateApiKey emits
        let upper = format!("tk_{}", "0123456789ABCDEF".repeat(4));
        assert!(!is_valid_token_format(&upper));
        // right prefix, wrong length
        let short = format!("tk_{}", "abcd".repeat(15));
        assert!(!is_valid_token_format(&short));
        // non-hex chars
        let bad = format!("tk_{}", "zzzz6789abcdef01".repeat(4));
        assert!(!is_valid_token_format(&bad));
    }

    #[test]
    fn fingerprint_shows_edges_only() {
        let token = format!("tk_{}", "0123456789abcdef".repeat(4));
        let fp = token_fingerprint(&token);
        assert_eq!(fp, "tk_0123…cdef");
        assert!(!fp.contains(&token[8..60]));
    }

    #[test]
    fn status_mapping_matches_platform_taxonomy() {
        assert!(matches!(
            map_status(401, "Invalid API key".into(), None),
            ApiError::Unauthorized { .. }
        ));
        assert!(matches!(
            map_status(403, "auto-disabled".into(), None),
            ApiError::Forbidden { .. }
        ));
        assert!(matches!(
            map_status(404, "x".into(), None),
            ApiError::NotFound { .. }
        ));
        assert!(matches!(
            map_status(409, "x".into(), None),
            ApiError::Conflict { .. }
        ));
        match map_status(429, "Too many requests".into(), Some(42)) {
            ApiError::RateLimited { retry_after_secs } => assert_eq!(retry_after_secs, 42),
            other => panic!("expected rate limited, got {other:?}"),
        }
        match map_status(429, "Too many requests".into(), None) {
            ApiError::RateLimited { retry_after_secs } => assert_eq!(retry_after_secs, 60),
            other => panic!("expected rate limited, got {other:?}"),
        }
        assert!(matches!(
            map_status(503, "x".into(), None),
            ApiError::Server { .. }
        ));
        assert!(matches!(
            map_status(418, "x".into(), None),
            ApiError::Other { .. }
        ));
    }

    #[test]
    fn base_url_normalization() {
        assert_eq!(
            normalize_base_url("https://tragentics.com/").unwrap(),
            "https://tragentics.com"
        );
        assert_eq!(
            normalize_base_url("http://localhost:4571").unwrap(),
            "http://localhost:4571"
        );
        assert_eq!(
            normalize_base_url("http://127.0.0.1:4571/").unwrap(),
            "http://127.0.0.1:4571"
        );
        assert!(normalize_base_url("http://example.com").is_err());
        assert!(normalize_base_url("ftp://x").is_err());
        assert!(normalize_base_url("").is_err());
        assert!(normalize_base_url("https://").is_err());
    }

    #[test]
    fn me_agent_parses_platform_shape() {
        // Exact field set returned by app/api/agents/me/route.ts
        let body = r#"{
            "id":"3f2c1a2b-0000-4000-8000-000000000001",
            "name":"Invoice Parser",
            "slug":"invoice-parser",
            "status":"offline",
            "is_public":false,
            "is_revoked":false,
            "is_archived":false,
            "last_heartbeat":null,
            "created_at":"2026-07-30T12:00:00.000Z"
        }"#;
        let parsed: MeAgent = serde_json::from_str(body).unwrap();
        assert_eq!(parsed.name, "Invoice Parser");
        assert_eq!(parsed.status, "offline");
        assert!(parsed.last_heartbeat.is_none());
    }

    #[test]
    fn heartbeat_response_parses_platform_shape() {
        let body = r#"{
            "heartbeat":"accepted",
            "agent":{"id":"a","status":"online","last_heartbeat":"2026-07-31T01:02:03.000Z"}
        }"#;
        let parsed: HeartbeatResponse = serde_json::from_str(body).unwrap();
        assert_eq!(parsed.agent.status, "online");
        assert_eq!(parsed.heartbeat, "accepted");
    }
}
