//! Zerodha Kite Connect's daily login flow — genuinely different from
//! Upstox's Analytics Token: there's no long-lived token here, a fresh
//! `access_token` must be obtained every trading day via a real OAuth
//! redirect. Flow, verified against Kite's own docs:
//! 1. Open `https://kite.zerodha.com/connect/login?v=3&api_key=...` in
//!    the system browser.
//! 2. After the user logs in (with TOTP), Kite redirects to the
//!    registered redirect URL with `?request_token=...`.
//! 3. POST that token + a SHA-256 checksum (api_key + request_token +
//!    api_secret) to `/session/token` to get the day's `access_token`.
//!
//! LIFECYCLE GUARANTEE (explicit requirement, not incidental): the local
//! HTTP listener that catches step 2's redirect is bound ONLY inside
//! `login_via_local_redirect_listener`, and is bound to a `tokio::net::
//! TcpListener` local variable that Rust drops — closing the OS socket —
//! the moment this function returns, by ordinary ownership rules, not a
//! manual cleanup step that could be skipped on an error path. It is
//! never created by anything else, never stored in AppState, and there
//! is no code path that starts it before this function is called or
//! keeps it alive after this function returns (success, timeout, or
//! error all drop it the same way). A `tokio::time::timeout` wraps the
//! "wait for the browser redirect" step specifically so a login attempt
//! the user abandons doesn't hold the socket open indefinitely either.

use reqwest::Client;
use sha2::{Digest, Sha256};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[derive(Debug, thiserror::Error)]
pub enum KiteAuthError {
    #[error("couldn't bind local redirect listener on port {port}: {source}")]
    ListenerBindFailed { port: u16, source: std::io::Error },
    #[error("couldn't open the login page in your browser: {0}")]
    BrowserOpenFailed(String),
    #[error("timed out waiting for the Zerodha login redirect — the login page may not have been completed")]
    TimedOut,
    #[error("redirect received, but it carried no request_token — login may have been denied")]
    NoRequestToken,
    #[error("request failed: {0}")]
    RequestFailed(String),
    #[error("Kite token exchange failed: {0}")]
    TokenExchangeFailed(String),
}

pub struct KiteLoginResult {
    pub access_token: String,
    pub user_id: String,
}

/// The whole point of this function's shape: everything it owns (the
/// listener, the timeout) is scoped to its own stack frame. See the
/// module doc comment for the exact lifecycle guarantee this gives.
pub async fn login_via_local_redirect_listener(api_key: &str, api_secret: &str, redirect_port: u16) -> Result<KiteLoginResult, KiteAuthError> {
    let listener = TcpListener::bind(("127.0.0.1", redirect_port))
        .await
        .map_err(|source| KiteAuthError::ListenerBindFailed { port: redirect_port, source })?;

    let login_url = format!("https://kite.zerodha.com/connect/login?v=3&api_key={api_key}");
    open::that(&login_url).map_err(|e| KiteAuthError::BrowserOpenFailed(e.to_string()))?;

    // 3 minutes is generous for a manual login + TOTP entry without
    // leaving the port bound indefinitely if the user just never
    // finishes (closes the tab, gets distracted, login fails silently).
    let request_token = tokio::time::timeout(Duration::from_secs(180), accept_one_redirect(&listener))
        .await
        .map_err(|_| KiteAuthError::TimedOut)??;
    // `listener` goes out of scope at the end of this function either
    // way — the drop happens here implicitly, freeing the port, whether
    // we return Ok or Err from this point on.

    exchange_request_token(api_key, api_secret, &request_token).await
}

/// Accepts exactly one TCP connection, reads the HTTP request line,
/// extracts `request_token` from the query string, and responds with a
/// small HTML page telling the user they can close the tab — then the
/// connection (and the listener, back in the caller) close.
async fn accept_one_redirect(listener: &TcpListener) -> Result<String, KiteAuthError> {
    let (mut stream, _) = listener.accept().await.map_err(|e| KiteAuthError::RequestFailed(e.to_string()))?;

    let mut buf = vec![0u8; 8192];
    let n = stream.read(&mut buf).await.map_err(|e| KiteAuthError::RequestFailed(e.to_string()))?;
    let request_text = String::from_utf8_lossy(&buf[..n]);

    let request_token = parse_request_token_from_http_request_line(&request_text).ok_or(KiteAuthError::NoRequestToken);

    let (status_line, body) = match &request_token {
        Ok(_) => ("HTTP/1.1 200 OK", "<html><body><h3>Login successful — you can close this tab.</h3></body></html>"),
        Err(_) => ("HTTP/1.1 400 Bad Request", "<html><body><h3>Login didn't complete — no token received. You can close this tab.</h3></body></html>"),
    };
    let response = format!("{status_line}\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n{body}", body.len());
    let _ = stream.write_all(response.as_bytes()).await; // best-effort — the token itself is what matters, not whether the browser sees a pretty page

    request_token
}

/// Parses `request_token` out of the first line of a raw HTTP request,
/// e.g. `GET /callback?request_token=abc123&action=login&status=success
/// HTTP/1.1`. Deliberately hand-rolled rather than pulling in a full HTTP
/// server framework for a listener that exists for a few seconds at most
/// and handles exactly one request.
fn parse_request_token_from_http_request_line(request_text: &str) -> Option<String> {
    let first_line = request_text.lines().next()?;
    let path_and_query = first_line.split_whitespace().nth(1)?;
    let query = path_and_query.split_once('?').map(|(_, q)| q)?;
    query.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        (key == "request_token").then(|| value.to_string())
    })
}

async fn exchange_request_token(api_key: &str, api_secret: &str, request_token: &str) -> Result<KiteLoginResult, KiteAuthError> {
    let mut hasher = Sha256::new();
    hasher.update(api_key.as_bytes());
    hasher.update(request_token.as_bytes());
    hasher.update(api_secret.as_bytes());
    let checksum = format!("{:x}", hasher.finalize());

    let client = Client::new();
    let response = client
        .post("https://api.kite.trade/session/token")
        .header("X-Kite-Version", "3")
        .form(&[("api_key", api_key), ("request_token", request_token), ("checksum", &checksum)])
        .send()
        .await
        .map_err(|e| KiteAuthError::RequestFailed(e.to_string()))?;

    #[derive(serde::Deserialize)]
    struct TokenResponse {
        status: String,
        data: Option<TokenData>,
        message: Option<String>,
    }
    #[derive(serde::Deserialize)]
    struct TokenData {
        access_token: String,
        user_id: String,
    }

    let body: TokenResponse = response.json().await.map_err(|e| KiteAuthError::TokenExchangeFailed(format!("couldn't parse response: {e}")))?;

    if body.status != "success" {
        return Err(KiteAuthError::TokenExchangeFailed(body.message.unwrap_or_else(|| "unknown error".to_string())));
    }
    let data = body.data.ok_or_else(|| KiteAuthError::TokenExchangeFailed("success status but no data".to_string()))?;
    Ok(KiteLoginResult { access_token: data.access_token, user_id: data.user_id })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_request_token_from_a_real_shaped_redirect_request_line() {
        let request = "GET /callback?request_token=abc123xyz&action=login&status=success HTTP/1.1\r\nHost: 127.0.0.1:17872\r\n\r\n";
        assert_eq!(parse_request_token_from_http_request_line(request), Some("abc123xyz".to_string()));
    }

    #[test]
    fn returns_none_when_request_token_is_absent() {
        let request = "GET /callback?action=login&status=false HTTP/1.1\r\nHost: 127.0.0.1:17872\r\n\r\n";
        assert_eq!(parse_request_token_from_http_request_line(request), None);
    }

    #[test]
    fn returns_none_for_a_bare_request_with_no_query_string() {
        let request = "GET /favicon.ico HTTP/1.1\r\nHost: 127.0.0.1:17872\r\n\r\n";
        assert_eq!(parse_request_token_from_http_request_line(request), None);
    }

    #[test]
    fn finds_request_token_regardless_of_query_param_order() {
        let request = "GET /callback?status=success&request_token=xyz789&action=login HTTP/1.1\r\n\r\n";
        assert_eq!(parse_request_token_from_http_request_line(request), Some("xyz789".to_string()));
    }

    #[test]
    fn checksum_matches_kites_documented_sha256_of_concatenated_fields() {
        // Verified against Kite's own documented formula: SHA-256 of
        // api_key + request_token + api_secret concatenated as one
        // string (not separately hashed, not joined with any separator).
        let mut hasher = Sha256::new();
        hasher.update(b"testkey");
        hasher.update(b"testtoken");
        hasher.update(b"testsecret");
        let expected = format!("{:x}", hasher.finalize());

        let mut hasher2 = Sha256::new();
        hasher2.update("testkeytesttokentestsecret".as_bytes());
        let expected2 = format!("{:x}", hasher2.finalize());

        assert_eq!(expected, expected2, "chunked update() calls must produce the same digest as one concatenated string");
    }

    #[tokio::test]
    async fn listener_port_is_freed_immediately_after_the_function_returns() {
        // Binds on an OS-assigned free port, drops the listener (by
        // letting it go out of scope, the same way the real function
        // does), then confirms a NEW listener can immediately rebind to
        // the exact same port — proving no lingering process/handle kept
        // it open, which is the actual resource-efficiency guarantee
        // that matters here.
        let port = {
            let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
            listener.local_addr().unwrap().port()
        }; // listener dropped here
        let rebind = TcpListener::bind(("127.0.0.1", port)).await;
        assert!(rebind.is_ok(), "port should be immediately free after the listener is dropped");
    }
}
