//! The playground's MCP endpoint as a client sees it: JSON-RPC messages POSTed to
//! `/mcp/<blueprint>` on the server listener, each reply read as it arrives. The
//! stand-in's bridge relays through it, and own-key chat calls tools with it. One
//! [`McpHttp`] is one MCP session.

use std::collections::BTreeMap;
use std::io::Read as _;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use base64::Engine as _;
use serde_json::Value;

use super::client::read_feed;

const SESSION_HEADER: &str = "mcp-session-id";
const PROTOCOL_HEADER: &str = "mcp-protocol-version";
const SECRETS_HEADER: &str = "submilli-secrets";

/// Connecting is bounded; a reply is not, since a tool call lasts as long as its run.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// The most of a refusal's text kept for the message that reports it.
const MAX_REFUSAL_BYTES: u64 = 64 * 1024;
/// The largest JSON reply read whole.
const MAX_REPLY_BYTES: u64 = 64 << 20;
/// How long ending the session may take: it is best effort, on the way out.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(2);

/// Why a message got no reply.
#[derive(Debug)]
pub(crate) enum Failure {
    /// The endpoint answered with an error status, and this text.
    Refused { status: u16, text: String },
    /// The server no longer knows the session: the playground restarted, or the session
    /// expired.
    SessionGone,
    /// No answer: the playground stopped or was never reached.
    Unanswered(String),
    /// The reply was cut off after the message arrived, so a call may have run.
    Lost(String),
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused { status, text } if text.is_empty() => {
                write!(f, "the playground refused the request (HTTP {status})")
            }
            Self::Refused { status, text } => {
                write!(
                    f,
                    "the playground refused the request (HTTP {status}): {text}"
                )
            }
            Self::SessionGone => write!(
                f,
                "the playground no longer has this MCP session (it restarted, or the \
                 session expired); restart this MCP server to connect again"
            ),
            Self::Unanswered(error) => write!(f, "the playground did not answer: {error}"),
            Self::Lost(error) => write!(
                f,
                "the playground's reply was cut off, so the call may have run: {error}"
            ),
        }
    }
}

pub(crate) struct McpHttp {
    agent: ureq::Agent,
    url: String,
    token: String,
    /// The `submilli-secrets` header, sent with `initialize` only: the server binds a
    /// session's secrets when it starts.
    secrets: Option<String>,
    /// From the server's answer to `initialize`, sent with every later message.
    session: Mutex<Option<String>>,
    protocol: Mutex<Option<String>>,
}

impl McpHttp {
    pub(crate) fn new(
        server_port: u16,
        blueprint: &str,
        token: &str,
        secrets: &BTreeMap<String, String>,
    ) -> Self {
        let secrets = (!secrets.is_empty()).then(|| {
            let json = serde_json::to_vec(secrets).unwrap_or_default();
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json)
        });
        Self {
            agent: ureq::Agent::config_builder()
                .max_redirects(0)
                .http_status_as_error(false)
                .timeout_connect(Some(CONNECT_TIMEOUT))
                .build()
                .into(),
            url: format!("http://127.0.0.1:{server_port}/mcp/{blueprint}"),
            token: token.to_owned(),
            secrets,
            session: Mutex::new(None),
            protocol: Mutex::new(None),
        }
    }

    /// Carries on MCP session `session`, which an earlier connection started under
    /// `protocol`: later messages name it, with no `initialize` of their own.
    pub(crate) fn resume(&self, session: &str, protocol: &str) {
        *lock(&self.session) = Some(session.to_owned());
        *lock(&self.protocol) = Some(protocol.to_owned());
    }

    /// The MCP session id the server gave, once `initialize` was answered.
    pub(crate) fn session(&self) -> Option<String> {
        lock(&self.session).clone()
    }

    /// Sends one JSON-RPC message and hands each message of the reply to `each` as it
    /// arrives. A notification or a response is answered with none.
    pub(crate) fn send(&self, message: &Value, mut each: impl FnMut(Value)) -> Result<(), Failure> {
        let initialize = message["method"] == "initialize";
        let mut builder = ureq::http::Request::builder()
            .method("POST")
            .uri(&self.url)
            .header("authorization", format!("Bearer {}", self.token))
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream");
        if let Some(session) = self.session() {
            builder = builder.header(SESSION_HEADER, session);
        }
        if let Some(protocol) = lock(&self.protocol).clone() {
            builder = builder.header(PROTOCOL_HEADER, protocol);
        }
        if initialize && let Some(secrets) = &self.secrets {
            builder = builder.header(SECRETS_HEADER, secrets);
        }
        let request = builder
            .body(message.to_string())
            .map_err(|error| Failure::Unanswered(error.to_string()))?;
        let mut response = self
            .agent
            .run(request)
            .map_err(|error| Failure::Unanswered(error.to_string()))?;
        let status = response.status();
        if let Some(session) = header(&response, SESSION_HEADER) {
            *lock(&self.session) = Some(session);
        }
        if status.as_u16() == 202 {
            return Ok(());
        }
        if !status.is_success() {
            // The start of a long refusal is kept; one that cannot be read at all is not
            // taken for an empty one.
            let mut bytes = Vec::new();
            let read = response
                .body_mut()
                .with_config()
                .limit(u64::MAX)
                .reader()
                .take(MAX_REFUSAL_BYTES)
                .read_to_end(&mut bytes);
            let text = String::from_utf8_lossy(&bytes).into_owned();
            // MCP's answer to a session id the server does not know; a missing blueprint
            // is a 404 too, and says so.
            if status.as_u16() == 404
                && !initialize
                && self.session().is_some()
                && ((read.is_ok() && text.trim().is_empty()) || text.contains("Session not found"))
            {
                return Err(Failure::SessionGone);
            }
            return Err(Failure::Refused {
                status: status.as_u16(),
                text: refusal_text(&text),
            });
        }
        let mut each = |reply: Value| {
            if initialize && let Some(protocol) = reply["result"]["protocolVersion"].as_str() {
                *lock(&self.protocol) = Some(protocol.to_owned());
            }
            each(reply);
        };
        let streamed = header(&response, "content-type")
            .is_some_and(|kind| kind.starts_with("text/event-stream"));
        if streamed {
            let reader =
                std::io::BufReader::new(response.body_mut().with_config().limit(u64::MAX).reader());
            return read_feed(reader, |event| {
                if let Ok(reply) = serde_json::from_str::<Value>(&event.data) {
                    each(reply);
                }
                true
            })
            .map_err(|error| Failure::Lost(error.to_string()));
        }
        let body = response
            .body_mut()
            .with_config()
            .limit(MAX_REPLY_BYTES)
            .read_to_string()
            .map_err(|error| Failure::Lost(error.to_string()))?;
        if body.trim().is_empty() {
            return Ok(());
        }
        match serde_json::from_str::<Value>(&body) {
            Ok(Value::Array(replies)) => replies.into_iter().for_each(&mut each),
            Ok(reply) => each(reply),
            Err(error) => {
                return Err(Failure::Lost(format!("the reply is not JSON: {error}")));
            }
        }
        Ok(())
    }

    /// Ends the server's session, if one started. Best effort: the server also expires
    /// sessions nobody uses.
    pub(crate) fn close(&self) {
        let Some(session) = lock(&self.session).take() else {
            return;
        };
        let request = ureq::http::Request::builder()
            .method("DELETE")
            .uri(&self.url)
            .header("authorization", format!("Bearer {}", self.token))
            .header(SESSION_HEADER, session)
            .body(());
        if let Ok(request) = request {
            let request = self
                .agent
                .configure_request(request)
                .timeout_global(Some(CLOSE_TIMEOUT))
                .build();
            let _ = self.agent.run(request);
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    // Only ids are kept here; one a panic left half-set is still a whole string.
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn header(response: &ureq::http::Response<ureq::Body>, name: &str) -> Option<String> {
    response
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

/// A refusal's own words: the message of a JSON error body, or its text.
fn refusal_text(body: &str) -> String {
    let parsed: Option<Value> = serde_json::from_str(body).ok();
    let message = parsed.as_ref().and_then(|body| {
        body["error"]["message"]
            .as_str()
            .or_else(|| body["message"].as_str())
            .or_else(|| body["error"].as_str())
    });
    message.unwrap_or(body).trim().to_owned()
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use serde_json::json;

    use super::*;

    /// A server on loopback that answers each request with the next of `responses`, and
    /// hands back each request's head and body.
    fn answering(responses: Vec<String>) -> (McpHttp, std::sync::mpsc::Receiver<String>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (sent, received) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for response in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = Vec::new();
                let mut byte = [0_u8; 1];
                while !request.ends_with(b"\r\n\r\n") && stream.read(&mut byte).unwrap_or(0) == 1 {
                    request.push(byte[0]);
                }
                let head = String::from_utf8_lossy(&request).to_lowercase();
                let length = head
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length: "))
                    .and_then(|value| value.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                let mut body = vec![0_u8; length];
                stream.read_exact(&mut body).unwrap();
                sent.send(format!("{head}{}", String::from_utf8_lossy(&body)))
                    .unwrap();
                stream.write_all(response.as_bytes()).unwrap();
            }
        });
        let secrets = BTreeMap::from([("API_KEY".to_owned(), "dummy-value".to_owned())]);
        (McpHttp::new(port, "demo", "tok", &secrets), received)
    }

    fn reply(status: &str, headers: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\n{headers}content-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    fn collect(connection: &McpHttp, message: &Value) -> (Vec<Value>, Result<(), Failure>) {
        let mut replies = Vec::new();
        let sent = connection.send(message, |reply| replies.push(reply));
        (replies, sent)
    }

    #[test]
    fn a_session_starts_with_secrets_and_later_messages_carry_its_ids() {
        let initialized = json!({
            "jsonrpc": "2.0", "id": 1,
            "result": { "protocolVersion": "2025-06-18", "capabilities": {} }
        });
        let (connection, requests) = answering(vec![
            reply(
                "200 OK",
                "content-type: text/event-stream\r\nmcp-session-id: s-1\r\n",
                &format!("data: {initialized}\n\n"),
            ),
            reply("202 Accepted", "", ""),
        ]);
        let (replies, sent) = collect(
            &connection,
            &json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} }),
        );
        sent.unwrap();
        assert_eq!(replies, [initialized]);
        let first = requests.recv().unwrap();
        assert!(first.starts_with("post /mcp/demo "), "{first}");
        assert!(first.contains("authorization: bearer tok"), "{first}");
        let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(br#"{"API_KEY":"dummy-value"}"#)
            .to_lowercase();
        assert!(
            first.contains(&format!("submilli-secrets: {encoded}")),
            "{first}"
        );
        assert_eq!(connection.session().as_deref(), Some("s-1"));

        let (replies, sent) = collect(
            &connection,
            &json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
        );
        sent.unwrap();
        assert!(replies.is_empty());
        let second = requests.recv().unwrap();
        assert!(second.contains("mcp-session-id: s-1"), "{second}");
        assert!(
            second.contains("mcp-protocol-version: 2025-06-18"),
            "{second}"
        );
        assert!(!second.contains("submilli-secrets"), "{second}");
    }

    #[test]
    fn a_refusal_reports_its_status_and_message() {
        let (connection, _requests) = answering(vec![reply(
            "400 Bad Request",
            "content-type: application/json\r\n",
            r#"{"error":{"message":"missing required variable customerId"}}"#,
        )]);
        let (replies, sent) = collect(
            &connection,
            &json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} }),
        );
        assert!(replies.is_empty());
        let failure = sent.unwrap_err();
        assert!(matches!(failure, Failure::Refused { status: 400, .. }));
        assert_eq!(
            failure.to_string(),
            "the playground refused the request (HTTP 400): missing required variable customerId"
        );
    }

    #[test]
    fn a_json_reply_is_handed_on_whole() {
        let answer = json!({ "jsonrpc": "2.0", "id": 7, "result": { "tools": [] } });
        let (connection, _requests) = answering(vec![reply(
            "200 OK",
            "content-type: application/json\r\n",
            &answer.to_string(),
        )]);
        let (replies, sent) = collect(
            &connection,
            &json!({ "jsonrpc": "2.0", "id": 7, "method": "tools/list" }),
        );
        sent.unwrap();
        assert_eq!(replies, [answer]);
    }

    #[test]
    fn a_session_the_server_forgot_says_to_restart_and_a_cut_reply_may_have_run() {
        let initialized = json!({ "jsonrpc": "2.0", "id": 1, "result": {} });
        let (connection, _requests) = answering(vec![
            reply(
                "200 OK",
                "content-type: application/json\r\nmcp-session-id: s-1\r\n",
                &initialized.to_string(),
            ),
            reply("404 Not Found", "", "Not Found: Session not found"),
            reply("404 Not Found", "", "unknown blueprint: demo"),
            reply("404 Not Found", "", ""),
            reply(
                "404 Not Found",
                "",
                &format!("unknown blueprint: {}", "x".repeat(70_000)),
            ),
            // Promises more than it sends, then closes.
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 100\r\n\
             connection: close\r\n\r\n{\"jsonrpc\""
                .to_owned(),
        ]);
        let ping = json!({ "jsonrpc": "2.0", "id": 2, "method": "ping" });
        collect(
            &connection,
            &json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize" }),
        )
        .1
        .unwrap();
        let gone = collect(&connection, &ping).1.unwrap_err();
        assert!(matches!(gone, Failure::SessionGone), "{gone}");
        assert!(
            gone.to_string().contains("restart this MCP server"),
            "{gone}"
        );
        let missing = collect(&connection, &ping).1.unwrap_err();
        assert!(
            matches!(missing, Failure::Refused { status: 404, .. }),
            "{missing}"
        );
        assert!(
            missing.to_string().contains("unknown blueprint"),
            "{missing}"
        );
        let empty = collect(&connection, &ping).1.unwrap_err();
        assert!(matches!(empty, Failure::SessionGone), "{empty}");
        // Too long to read whole: its start is kept, and it is no session's loss.
        let long = collect(&connection, &ping).1.unwrap_err();
        assert!(
            matches!(&long, Failure::Refused { status: 404, text }
                if text.starts_with("unknown blueprint")),
            "{long}"
        );
        let lost = collect(&connection, &ping).1.unwrap_err();
        assert!(matches!(lost, Failure::Lost(_)), "{lost}");
        assert!(lost.to_string().contains("may have run"), "{lost}");
    }

    #[test]
    fn no_listener_is_unanswered() {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let connection = McpHttp::new(port, "demo", "tok", &BTreeMap::new());
        let (_, sent) = collect(
            &connection,
            &json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" }),
        );
        assert!(matches!(sent, Err(Failure::Unanswered(_))));
    }
}
