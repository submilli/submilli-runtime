//! Operator-side OAuth driver for `submilli server mcp authenticate`.
//!
//! The submilli **server** owns the OAuth client (config, discovery, the
//! secret-bearing token exchange). The CLI only: asks the server for the resolved
//! client config, runs the PKCE loopback flow in the operator's browser, captures
//! the authorization `code`, and forwards it to the server to exchange. **No client
//! secret ever touches the CLI.**

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;

use anyhow::{Context, Result, bail};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// The bits of the server's `GET …/auth-config` response the CLI needs to build
/// the authorize URL (other fields, like the token endpoint, are the server's
/// concern and ignored here). Never includes a secret.
#[derive(Debug, Deserialize)]
pub struct AuthConfig {
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub authorization_endpoint: Option<String>,
    #[serde(default)]
    pub scopes: Vec<String>,
}

/// A PKCE verifier + its S256 challenge.
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

pub fn pkce() -> Result<Pkce> {
    let verifier = random_token(32)?;
    Ok(Pkce {
        challenge: s256_challenge(&verifier),
        verifier,
    })
}

/// The base64url-no-pad SHA-256 of the verifier (RFC 7636 S256).
pub fn s256_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// `nbytes` of OS randomness as a base64url-no-pad string.
pub fn random_token(nbytes: usize) -> Result<String> {
    let mut buf = vec![0u8; nbytes];
    getrandom::getrandom(&mut buf).map_err(|e| anyhow::anyhow!("getrandom failed: {e}"))?;
    Ok(URL_SAFE_NO_PAD.encode(buf))
}

/// Build the `authorization_endpoint?…` URL for the auth-code + PKCE flow.
pub fn authorize_url(
    authorization_endpoint: &str,
    client_id: &str,
    scopes: &[String],
    redirect_uri: &str,
    challenge: &str,
    state: &str,
) -> String {
    let mut params = vec![
        ("response_type", "code"),
        ("client_id", client_id),
        ("redirect_uri", redirect_uri),
        ("state", state),
        ("code_challenge", challenge),
        ("code_challenge_method", "S256"),
    ];
    let scope = scopes.join(" ");
    if !scope.is_empty() {
        params.push(("scope", scope.as_str()));
    }
    let sep = if authorization_endpoint.contains('?') {
        '&'
    } else {
        '?'
    };
    format!("{authorization_endpoint}{sep}{}", query_string(&params))
}

/// Accept one redirect on the loopback listener, validate `state`, and return the
/// authorization code.
pub fn wait_for_code(listener: &TcpListener, expected_state: &str) -> Result<String> {
    let (mut stream, _) = listener
        .accept()
        .context("waiting for the OAuth redirect")?;
    let mut buf = [0u8; 4096];
    let n = stream
        .read(&mut buf)
        .context("reading the redirect request")?;
    let request = String::from_utf8_lossy(&buf[..n]);
    let target = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("");
    let query = target.split_once('?').map_or("", |(_, q)| q);
    let params = parse_query(query);

    let _ = write_browser_response(&mut stream, params.contains_key("code"));

    if let Some(error) = params.get("error") {
        bail!("authorization failed: {error}");
    }
    if params.get("state").map(String::as_str) != Some(expected_state) {
        bail!("OAuth state mismatch — aborting (possible CSRF or a stale redirect)");
    }
    params
        .get("code")
        .cloned()
        .context("the redirect carried no authorization code")
}

fn write_browser_response(stream: &mut std::net::TcpStream, ok: bool) -> std::io::Result<()> {
    let body = if ok {
        "Submilli: authorization received. You may close this tab."
    } else {
        "Submilli: authorization failed. Check the terminal."
    };
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn query_string(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", urlencode(k), urlencode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// Percent-encode everything outside the RFC 3986 unreserved set.
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn parse_query(query: &str) -> HashMap<String, String> {
    query
        .split('&')
        .filter(|p| !p.is_empty())
        .filter_map(|pair| {
            let (k, v) = pair.split_once('=')?;
            Some((percent_decode(k), percent_decode(v)))
        })
        .collect()
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                if let Ok(byte) = u8::from_str_radix(hex, 16) {
                    out.push(byte);
                    i += 3;
                } else {
                    out.push(b'%');
                    i += 1;
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            other => {
                out.push(other);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests;
