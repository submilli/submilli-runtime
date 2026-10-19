//! `submilli playground mcp`: the stand-in's bridge. A coding assistant's subagent
//! launches it as a stdio MCP server; it relays each JSON-RPC line to the running
//! playground's MCP endpoint with the `stand-in` token, the binding's variables, and the
//! development secrets, and writes each reply back as a line. Its runs are labeled
//! `stand-in`.
//!
//! Stdout is the protocol channel: everything else goes to stderr. When the bridge cannot
//! connect (no playground, or a required value unbound), it says why on stderr and
//! answers every request with that reason, so the assistant sees it instead of hanging.
//! It connects once: after the playground restarts, the assistant restarts it.

use std::collections::BTreeMap;
use std::io::{BufRead, Write as _};
use std::process::ExitCode;
use std::sync::{Arc, Mutex, PoisonError};

use anyhow::Result;
use serde_json::{Value, json};

use super::controls::act::{Answer, connect_as};
use super::controls::render::{EXIT_FAILURE, EXIT_SUCCESS};
use super::mcp_http::{Failure, McpHttp};
use super::state::STAND_IN_TOKEN;

/// JSON-RPC's code for an error the server defines; the message says what it was.
const SERVER_ERROR: i64 = -32000;
const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;

pub(crate) fn execute() -> Result<ExitCode> {
    let (connection, binding) = match open() {
        Ok(opened) => opened,
        Err((exit, reason)) => return Ok(refuse(&reason, exit)),
    };
    let connection = Arc::new(connection);
    stop_on_signal(&connection);
    let out = Output::default();
    let variables = binding.variables;
    let mut in_flight = Vec::new();
    for mut message in messages(std::io::stdin().lock(), &out) {
        if message["method"] == "initialize" {
            with_variables(&mut message, &variables);
            relay_initialize(&connection, &out, &message);
            continue;
        }
        // A request may wait on a run, so it is relayed on its own thread: a cancellation
        // or another request is not held up behind it. A notification goes in order.
        if message.get("id").is_none() {
            relay(&connection, &out, &message);
        } else {
            let connection = Arc::clone(&connection);
            let out = out.clone();
            in_flight.push(std::thread::spawn(move || {
                relay(&connection, &out, &message);
            }));
            in_flight.retain(|request: &std::thread::JoinHandle<()>| !request.is_finished());
        }
    }
    // The client may close stdin right after its last request: answer what it asked
    // before ending the session. A signal still ends it at once.
    for request in in_flight {
        let _ = request.join();
    }
    connection.close();
    Ok(ExitCode::SUCCESS)
}

/// What the bridge connects with, as the playground gives it.
#[derive(serde::Deserialize)]
struct Binding {
    blueprint: String,
    #[serde(default)]
    variables: BTreeMap<String, String>,
    #[serde(default)]
    secrets: BTreeMap<String, String>,
}

/// A connection to the running playground's MCP endpoint, or why there is none.
fn open() -> Result<(McpHttp, Binding), (u8, String)> {
    let failed = |error: anyhow::Error| (EXIT_FAILURE, format!("{error:#}"));
    let running = match connect_as(STAND_IN_TOKEN).map_err(failed)? {
        Ok(running) => running,
        Err(answer) => return Err(refusal(&answer)),
    };
    let answer: Answer = running
        .send("GET", "/api/bridge/binding", None)
        .map_err(failed)?;
    if answer.exit != EXIT_SUCCESS {
        return Err(refusal(&answer));
    }
    let binding: Binding = serde_json::from_value(answer.result).map_err(|error| {
        failed(anyhow::anyhow!(
            "the playground's binding is malformed: {error}"
        ))
    })?;
    let connection = McpHttp::new(
        running.record.server_port,
        &binding.blueprint,
        running.token(),
        &binding.secrets,
    );
    Ok((connection, binding))
}

fn refusal(answer: &Answer) -> (u8, String) {
    let reason = answer.result["error"]["message"]
        .as_str()
        .map_or_else(|| answer.notes.join("\n"), str::to_owned);
    (answer.exit, reason)
}

/// Says why on stderr, then answers every request with it until stdin closes.
fn refuse(reason: &str, exit: u8) -> ExitCode {
    eprintln!("submilli playground mcp: {reason}");
    // The client cannot reconnect a stdio server itself.
    let reason = format!("{reason} (then restart this MCP server)");
    let out = Output::default();
    for message in messages(std::io::stdin().lock(), &out) {
        if let Some(id) = message.get("id")
            && message.get("method").is_some()
        {
            out.write(&error_reply(id, SERVER_ERROR, &reason));
        }
    }
    ExitCode::from(exit)
}

/// The JSON-RPC messages on stdin, one a line, until it closes or fails. A line that is
/// not one is answered with why, and reading goes on.
fn messages<'a>(input: impl BufRead + 'a, out: &'a Output) -> impl Iterator<Item = Value> + 'a {
    lines(input).filter_map(move |line| match parsed(&line) {
        Line::Message(message) => Some(message),
        Line::Answered(error) => {
            out.write(&error);
            None
        }
        Line::Blank => None,
    })
}

/// Stdin's lines as bytes, until it closes or fails.
fn lines(input: impl BufRead) -> impl Iterator<Item = Vec<u8>> {
    input.split(b'\n').map_while(|line| {
        line.map_err(|error| eprintln!("submilli playground mcp: reading stdin failed: {error}"))
            .ok()
    })
}

/// What one line of stdin holds.
#[derive(Debug, PartialEq)]
enum Line {
    Blank,
    Message(Value),
    /// Not one message: the error reply that answers it.
    Answered(Value),
}

fn parsed(line: &[u8]) -> Line {
    if line.trim_ascii().is_empty() {
        return Line::Blank;
    }
    let Ok(message) = serde_json::from_slice::<Value>(line) else {
        return Line::Answered(error_reply(
            &Value::Null,
            PARSE_ERROR,
            "the line is not JSON",
        ));
    };
    if !message.is_object() {
        // MCP has no batches since 2025-06-18.
        return Line::Answered(error_reply(
            &Value::Null,
            INVALID_REQUEST,
            "a line must hold one JSON-RPC message",
        ));
    }
    Line::Message(message)
}

/// Relays `initialize`, telling the agent in the server's instructions which variables
/// the session holds, as an application tells its agent who it serves.
fn relay_initialize(connection: &McpHttp, out: &Output, message: &Value) {
    let sent: BTreeMap<String, String> = message["params"]["_meta"]["variables"]
        .as_object()
        .map(|variables| {
            variables
                .iter()
                .filter_map(|(name, value)| Some((name.clone(), value.as_str()?.to_owned())))
                .collect()
        })
        .unwrap_or_default();
    let note = super::variables_note(&sent);
    relay_with(connection, out, message, &|reply| {
        if let Some(note) = &note {
            add_to_instructions(reply, note);
        }
    });
}

/// Adds `note` to an `initialize` result's instructions.
fn add_to_instructions(reply: &mut Value, note: &str) {
    let Some(result) = reply.get_mut("result").and_then(Value::as_object_mut) else {
        return;
    };
    let instructions = match result.get("instructions").and_then(Value::as_str) {
        Some(given) if !given.trim().is_empty() => format!("{given}\n\n{note}"),
        _ => note.to_owned(),
    };
    result.insert("instructions".to_owned(), json!(instructions));
}

/// Sends `message` and writes each reply. A request whose reply never came is answered
/// with why, so the client is never left waiting.
fn relay(connection: &McpHttp, out: &Output, message: &Value) {
    relay_with(connection, out, message, &|_| {});
}

/// [`relay`], with `adjust` applied to the request's answer before it is written.
fn relay_with(connection: &McpHttp, out: &Output, message: &Value, adjust: &dyn Fn(&mut Value)) {
    let id = message
        .get("id")
        .filter(|_| message.get("method").is_some());
    let mut answered = false;
    let sent = connection.send(message, |mut reply| {
        let answer =
            id.is_some_and(|id| reply.get("id") == Some(id) && reply.get("method").is_none());
        if answer {
            adjust(&mut reply);
        }
        answered |= answer;
        out.write(&reply);
    });
    let Some(id) = id else {
        if let Err(failure) = sent {
            eprintln!("submilli playground mcp: {failure}");
        }
        return;
    };
    match sent {
        // The answer came before the reply broke off; the client has it.
        Err(failure) if answered => eprintln!("submilli playground mcp: {failure}"),
        // A restarted playground listens on a new port, which only a new bridge finds.
        Err(failure @ Failure::Unanswered(_)) => out.write(&error_reply(
            id,
            SERVER_ERROR,
            &format!("{failure}; if the playground restarted, restart this MCP server"),
        )),
        Err(failure) => out.write(&error_reply(id, SERVER_ERROR, &failure.to_string())),
        Ok(()) if !answered => out.write(&error_reply(
            id,
            SERVER_ERROR,
            "the playground ended its reply without an answer",
        )),
        Ok(()) => {}
    }
}

/// Puts the binding's variables in `initialize`'s `_meta.variables`, where the server
/// reads them; any value the client sent there is kept over the binding's.
fn with_variables(message: &mut Value, variables: &BTreeMap<String, String>) {
    if variables.is_empty() {
        return;
    }
    let Some(params) = message
        .as_object_mut()
        .map(|message| message.entry("params").or_insert_with(|| json!({})))
    else {
        return;
    };
    let Some(params) = params.as_object_mut() else {
        return;
    };
    let Some(meta) = params
        .entry("_meta")
        .or_insert_with(|| json!({}))
        .as_object_mut()
    else {
        return;
    };
    let Some(sent) = meta
        .entry("variables")
        .or_insert_with(|| json!({}))
        .as_object_mut()
    else {
        return;
    };
    for (name, value) in variables {
        sent.entry(name.clone()).or_insert_with(|| json!(value));
    }
}

fn error_reply(id: &Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// Stdout, one JSON-RPC message per line, whole lines only.
#[derive(Clone, Default)]
struct Output(Arc<Mutex<()>>);

impl Output {
    fn write(&self, message: &Value) {
        let _held = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let mut stdout = std::io::stdout().lock();
        // A closed stdout is the client gone; the loop ends when stdin closes too.
        let _ = writeln!(stdout, "{message}");
        let _ = stdout.flush();
    }
}

/// On SIGTERM or SIGINT (an assistant stops its MCP servers with a signal), ends the
/// server's session and exits.
fn stop_on_signal(connection: &Arc<McpHttp>) {
    let connection = Arc::clone(connection);
    std::thread::spawn(move || {
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .build()
        else {
            return;
        };
        runtime.block_on(async {
            use tokio::signal::unix::{SignalKind, signal};
            let (Ok(mut term), Ok(mut interrupt)) = (
                signal(SignalKind::terminate()),
                signal(SignalKind::interrupt()),
            ) else {
                return;
            };
            tokio::select! {
                _ = term.recv() => {}
                _ = interrupt.recv() => {}
            }
            connection.close();
            std::process::exit(0);
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_session_s_variables_are_added_to_the_server_s_instructions() {
        let variables = BTreeMap::from([("customerId".to_owned(), r#"cus_"x""#.to_owned())]);
        let note = crate::commands::playground::variables_note(&variables).unwrap();
        assert!(note.contains(r#"customerId = "cus_\"x\"""#), "{note}");
        let mut given = json!({ "id": 1, "result": { "instructions": "Use the tools." } });
        add_to_instructions(&mut given, &note);
        assert_eq!(
            given["result"]["instructions"],
            format!("Use the tools.\n\n{note}")
        );
        let mut bare = json!({ "id": 1, "result": {} });
        add_to_instructions(&mut bare, &note);
        assert_eq!(bare["result"]["instructions"], note);
        let mut failed = json!({ "id": 1, "error": { "code": 1, "message": "no" } });
        add_to_instructions(&mut failed, &note);
        assert!(failed.get("result").is_none());
        assert!(crate::commands::playground::variables_note(&BTreeMap::new()).is_none());
    }

    #[test]
    fn variables_go_in_initialize_meta_without_replacing_what_the_client_sent() {
        let mut message = json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": { "protocolVersion": "2025-06-18", "_meta": { "variables": { "a": "client" } } }
        });
        let variables = BTreeMap::from([
            ("a".to_owned(), "bound".to_owned()),
            ("customer; id=é".to_owned(), "x;y=é".to_owned()),
        ]);
        with_variables(&mut message, &variables);
        assert_eq!(
            message["params"]["_meta"]["variables"],
            json!({ "a": "client", "customer; id=é": "x;y=é" })
        );
        assert_eq!(message["params"]["protocolVersion"], "2025-06-18");

        let mut bare = json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize" });
        with_variables(&mut bare, &variables);
        assert_eq!(bare["params"]["_meta"]["variables"]["a"], "bound");
    }

    #[test]
    fn each_line_is_one_message_or_answered_with_why_and_reading_goes_on() {
        let input: &[u8] =
            b"{\"id\":1,\"method\":\"ping\"}\n\xff\xfe\n[{\"id\":2}]\n  \r\n{\"id\":3}";
        let read: Vec<Line> = lines(input).map(|line| parsed(&line)).collect();
        assert_eq!(read.len(), 5);
        assert_eq!(read[0], Line::Message(json!({ "id": 1, "method": "ping" })));
        let code = |line: &Line| match line {
            Line::Answered(error) => error["error"]["code"].as_i64(),
            _ => None,
        };
        assert_eq!(code(&read[1]), Some(PARSE_ERROR));
        assert_eq!(code(&read[2]), Some(INVALID_REQUEST));
        assert_eq!(read[3], Line::Blank);
        assert_eq!(read[4], Line::Message(json!({ "id": 3 })));
    }
}
