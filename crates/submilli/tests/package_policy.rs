//! Runs the first-party packages' policy scripts: each is a `main` program
//! executed under a restricted blueprint against the locally published package.
//!
//! `build test` runs package tests under an allow-all policy, so it cannot show
//! that a `main` caller is constrained. These scripts can, and they need no
//! credentials or network: each blueprint denies the package its secret or its
//! HTTP calls, so an allowed call stops at that gated operation.
//!
//! A check whose fields come from the service's answers cannot be reached that
//! way. `attio_status_rules_hold_against_a_fake_attio` and
//! `salesforce_owner_rules_hold_against_a_fake_salesforce` and
//! `zendesk_ticket_rules_hold_against_a_fake_zendesk` cover those: each runs a
//! copy of its package against a fake service on 127.0.0.1.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Packages that ship policy scripts under `tests/policy/`.
const PACKAGES: &[&str] = &[
    "attio",
    "browserbase-stagehand",
    "firecrawl",
    "github",
    "gmail",
    "google-calendar",
    "google-drive",
    "jina",
    "notion",
    "posthog",
    "salesforce",
    "slack-bot",
    "slack-user",
    "zendesk",
];

/// Required build settings, as `--config` gives them. The policy blueprints deny the
/// packages their secrets, so no request reaches these hosts.
const SETTINGS: &[(&str, &str)] = &[
    ("salesforce", SALESFORCE_ORG_SETTING),
    ("zendesk", ZENDESK_ACCOUNT_SETTING),
];

/// The org every build of `@submilli/salesforce` in the tests uses, and so the
/// host its checked-in `capabilities.yaml` names.
const SALESFORCE_ORG_SETTING: &str = "orgUrl=https://example.my.salesforce.com";

/// The account every build of `@submilli/zendesk` in the tests uses, and so the host its
/// checked-in `capabilities.yaml` names.
const ZENDESK_ACCOUNT_SETTING: &str = "accountUrl=https://example.zendesk.com";

#[test]
fn policy_scripts_pass_under_their_blueprints() {
    let home = tempfile::tempdir().expect("package store");
    put_secret(home.path(), "policy-token", "policy-placeholder");
    let mut failures = Vec::new();
    for package in PACKAGES {
        let name = format!("@submilli/{package}");
        let mut arguments = vec!["build", "publish-local", "-p", &name];
        if let Some((_, setting)) = SETTINGS.iter().find(|(with, _)| with == package) {
            arguments.extend(["--config", setting]);
        }
        let published = submilli(&arguments, home.path());
        if !published.status.success() {
            failures.push(format!(
                "publishing @submilli/{package} failed:\n{}",
                String::from_utf8_lossy(&published.stderr)
            ));
            continue;
        }
        let scripts = policy_scripts(package);
        if scripts.is_empty() {
            failures.push(format!("@submilli/{package} has no policy script"));
        }
        for script in scripts {
            let blueprint = script.with_extension("yaml");
            let run = submilli(
                &[
                    "run",
                    &script.to_string_lossy(),
                    "--blueprint",
                    &blueprint.to_string_lossy(),
                ],
                home.path(),
            );
            if !run.status.success() {
                failures.push(format!(
                    "{} failed:\n{}{}",
                    script.display(),
                    String::from_utf8_lossy(&run.stdout),
                    String::from_utf8_lossy(&run.stderr)
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

const ATTIO_DEAL: &str = "8a1b2c3d-1111-4222-8333-944455556666";
const ATTIO_KEY: &str = "fake-attio-key";

/// `records.setStatus` is checked with the canonical status title and the
/// record's previous status, which the package reads from Attio first, so an
/// offline script never reaches that check. Here a copy of `@submilli/attio`,
/// identical but for its API base, talks to a fake Attio, and
/// `tests/fake-host/status-rules.ts` runs as `main` under its blueprint: the
/// stage change is allowed and written, a change of another status attribute is
/// denied and not written, a status attribute passed as a plain value is
/// refused wherever it appears, and a company or person created with a status
/// is denied by the create-only rules.
#[test]
fn attio_status_rules_hold_against_a_fake_attio() {
    let fake = FakeHost::start(ATTIO_KEY, attio_answer);
    let home = tempfile::tempdir().expect("package store");
    put_secret(home.path(), "policy-token", ATTIO_KEY);
    let project = tempfile::tempdir().expect("package copy");
    copy_attio_with_base(
        project.path(),
        &format!("http://127.0.0.1:{}/v2", fake.port),
    );
    let published = Command::new(env!("CARGO_BIN_EXE_submilli"))
        .args(["build", "publish-local", "-p", "@submilli/attio"])
        .current_dir(project.path())
        .env("SUBMILLI_HOME", home.path())
        .env("SUBMILLI_TELEMETRY", "0")
        .output()
        .expect("publish the package copy");
    assert!(
        published.status.success(),
        "{}",
        String::from_utf8_lossy(&published.stderr)
    );

    let directory = repository_root().join("packages/attio/tests/fake-host");
    let script = directory.join("status-rules.ts");
    let blueprint = directory.join("status-rules.yaml");
    let run = submilli(
        &[
            "run",
            &script.to_string_lossy(),
            "--blueprint",
            &blueprint.to_string_lossy(),
        ],
        home.path(),
    );
    assert!(
        run.status.success(),
        "{}{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );

    let requests = fake.requests();
    assert!(
        !requests.is_empty(),
        "the package copy reached the fake Attio"
    );
    assert!(
        requests.iter().all(|request| request.authorized),
        "every request carries the key: {requests:#?}"
    );
    let writes: Vec<&FakeRequest> = requests
        .iter()
        .filter(|request| request.method != "GET")
        .collect();
    assert_eq!(
        writes.len(),
        1,
        "only the allowed stage change is written: {writes:#?}"
    );
    assert_eq!(writes[0].method, "PATCH");
    assert_eq!(
        writes[0].path,
        format!("/v2/objects/deals/records/{ATTIO_DEAL}")
    );
    assert_eq!(
        writes[0].body,
        serde_json::json!({ "data": { "values": { "stage": [{ "status": "s-won" }] } } })
    );
}

/// Copies the package's sources into `project` with its API base replaced.
fn copy_attio_with_base(project: &Path, base: &str) {
    const SHIPPED: &str = "const API = \"https://api.attio.com/v2\";";
    let source = repository_root().join("packages/attio");
    let target = project.join("packages/attio");
    for directory in ["src", "docs"] {
        std::fs::create_dir_all(target.join(directory)).expect("package directory");
        for entry in std::fs::read_dir(source.join(directory)).expect("package sources") {
            let path = entry.expect("source entry").path();
            let name = path.file_name().expect("a source file name");
            let mut text = std::fs::read_to_string(&path).expect("package source");
            if name == "lib.ts" {
                assert_eq!(
                    text.matches(SHIPPED).count(),
                    1,
                    "lib.ts declares the API base once"
                );
                text = text.replace(SHIPPED, &format!("const API = \"{base}\";"));
            }
            std::fs::write(target.join(directory).join(name), text).expect("copied source");
        }
    }
    std::fs::write(
        project.join("submilli.toml"),
        "[[package]]\nname = \"@submilli/attio\"\nversion = \"0.1.0\"\n\
         description = \"@submilli/attio against a fake Attio\"\nkeywords = [\"attio\"]\n\
         path = \"packages/attio\"\n",
    )
    .expect("manifest");
}

#[derive(Debug)]
struct FakeRequest {
    method: String,
    path: String,
    authorized: bool,
    body: serde_json::Value,
}

/// How a fake service answers a method, path and query string: a status line
/// and a JSON body, or none. A JSON string that starts with `<` is sent as XML,
/// for Salesforce's SOAP API.
type FakeAnswer = fn(&str, &str, &str) -> (&'static str, Option<serde_json::Value>);

/// A fake service on 127.0.0.1 that accepts one bearer token. It answers one
/// request per connection and records every request.
struct FakeHost {
    port: u16,
    requests: Arc<Mutex<Vec<FakeRequest>>>,
}

impl FakeHost {
    fn start(key: &'static str, answer: FakeAnswer) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind the fake host");
        let port = listener.local_addr().expect("fake host address").port();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&requests);
        // The thread serves until the test process exits.
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                serve(stream, &recorded, key, answer);
            }
        });
        FakeHost { port, requests }
    }

    fn requests(&self) -> Vec<FakeRequest> {
        // A poisoned lock means the serving thread panicked; the test fails either way.
        std::mem::take(&mut *self.requests.lock().expect("fake host log"))
    }
}

fn serve(stream: TcpStream, recorded: &Mutex<Vec<FakeRequest>>, key: &str, answer: FakeAnswer) {
    // A client that stops sending fails this request instead of stalling the server.
    if stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .is_err()
    {
        return;
    }
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let target = parts.next().unwrap_or_default().to_string();
    let mut length = 0;
    let mut authorized = false;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).is_err() || header.trim().is_empty() {
            break;
        }
        let (name, value) = header.split_once(':').unwrap_or((&header, ""));
        let value = value.trim();
        if name.eq_ignore_ascii_case("content-length") {
            length = value.parse().unwrap_or(0);
        }
        if name.eq_ignore_ascii_case("authorization") {
            authorized = value == format!("Bearer {key}");
        }
    }
    let mut body = vec![0; length];
    if reader.read_exact(&mut body).is_err() {
        return;
    }
    let text = String::from_utf8_lossy(&body).into_owned();
    // The SOAP API takes the token in the envelope's session header instead.
    authorized |= text.contains(&format!("<urn:sessionId>{key}</urn:sessionId>"));
    let (path, query) = target.split_once('?').unwrap_or((&target, ""));
    let (status, answer) = answer(&method, path, query);
    recorded.lock().expect("fake host log").push(FakeRequest {
        method,
        path: path.to_string(),
        authorized,
        body: if text.starts_with('<') {
            serde_json::Value::String(text)
        } else {
            serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null)
        },
    });
    let (kind, text) = match answer {
        Some(serde_json::Value::String(xml)) if xml.starts_with('<') => ("text/xml", xml),
        Some(json) => ("application/json", json.to_string()),
        None => ("application/json", String::new()),
    };
    let mut stream = reader.into_inner();
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\ncontent-type: {kind}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{text}",
        text.len()
    );
}

fn attio_answer(
    method: &str,
    path: &str,
    _query: &str,
) -> (&'static str, Option<serde_json::Value>) {
    let (status, body) = fake_attio(method, path);
    (status, Some(body))
}

fn fake_attio(method: &str, path: &str) -> (&'static str, serde_json::Value) {
    use serde_json::json;
    let status = |id: &str, title: &str| json!({ "id": { "status_id": id }, "title": title, "is_archived": false });
    let attribute = |slug: &str, kind: &str| {
        json!({ "id": { "attribute_id": format!("a-{slug}") }, "api_slug": slug, "title": slug, "type": kind,
            "is_multiselect": false, "is_writable": true, "is_archived": false })
    };
    let deal = |stage_id: &str, stage: &str| {
        json!({ "data": { "id": { "record_id": ATTIO_DEAL }, "created_at": "2026-10-26T10:00:00Z",
            "web_url": format!("https://app.attio.com/x/deals/{ATTIO_DEAL}"),
            "values": { "name": [{ "attribute_type": "text", "value": "Acme pilot" }],
                "stage": [{ "attribute_type": "status", "status": { "id": { "status_id": stage_id }, "title": stage } }] } } })
    };
    let record = format!("/v2/objects/deals/records/{ATTIO_DEAL}");
    match (method, path) {
        ("GET", "/v2/objects/deals/attributes/stage/statuses") => (
            "200 OK",
            json!({ "data": [status("s-lead", "Lead"), status("s-won", "Won 🎉"), status("s-lost", "Lost")] }),
        ),
        (
            "GET",
            "/v2/objects/companies/attributes/stage/statuses"
            | "/v2/objects/people/attributes/stage/statuses",
        ) => ("200 OK", json!({ "data": [status("c-lead", "Lead")] })),
        ("GET", "/v2/objects/deals/attributes/priority/statuses") => (
            "200 OK",
            json!({ "data": [status("p-high", "High"), status("p-low", "Low")] }),
        ),
        ("GET", "/v2/objects/deals/attributes") => (
            "200 OK",
            json!({ "data": [attribute("name", "text"), attribute("stage", "status"), attribute("priority", "status")] }),
        ),
        ("GET", path) if path == record => ("200 OK", deal("s-lead", "Lead")),
        ("PATCH", path) if path == record => ("200 OK", deal("s-won", "Won 🎉")),
        _ => (
            "404 Not Found",
            json!({ "status_code": 404, "type": "invalid_request_error", "code": "not_found", "message": "Not found" }),
        ),
    }
}

const SALESFORCE_TOKEN: &str = "fake-salesforce-token";
const SALESFORCE_API: &str = "/services/data/v68.0";
const SALESFORCE_SOAP: &str = "/services/Soap/u/68.0";
const SALESFORCE_ME: &str = "005000000000001AAA";
const SALESFORCE_OTHER: &str = "005000000000002AAA";
const OPPORTUNITY_MINE: &str = "006000000000001AAA";
const OPPORTUNITY_THEIRS: &str = "006000000000002AAA";
const LEAD_MINE: &str = "00Q000000000001EAA";
const LEAD_THEIRS: &str = "00Q000000000002EAA";
const WORK_ITEM: &str = "04i000000000001AAA";

/// Every write check of `@submilli/salesforce` that names an owner reads it from
/// Salesforce first, so an offline script never reaches it. Here a copy of the
/// package, pointed at a fake Salesforce, runs `tests/fake-host/owner-rules.ts`
/// under its blueprint: its own deal is read, renamed, moved to Closed Won and
/// deleted; another owner's deal, and a missing one, are denied, and nothing is
/// written to them.
#[test]
fn salesforce_owner_rules_hold_against_a_fake_salesforce() {
    let mine = format!("{SALESFORCE_API}/sobjects/Opportunity/{OPPORTUNITY_MINE}");
    let writes = salesforce_writes_against_a_fake("owner-rules");
    assert_eq!(
        writes,
        vec![
            (
                "PATCH".to_string(),
                mine.clone(),
                serde_json::json!({ "Name": "Acme renewal" })
            ),
            (
                "PATCH".to_string(),
                mine.clone(),
                serde_json::json!({ "StageName": "Closed Won" })
            ),
            ("DELETE".to_string(), mine, serde_json::Value::Null),
        ],
        "only the allowed writes reach Salesforce, all to the caller's own deal"
    );
}

/// The process and collaboration operations read the facts their checks need
/// (a record's owner and account, a lead's owner, an approval's process) from
/// Salesforce. `tests/fake-host/process-rules.ts` hands its own deal to a
/// queue, converts its own lead through the SOAP API, logs a call, approves a
/// discount and posts to the deal's feed; the same calls on another owner's
/// records, to a user rather than a queue, or mentioning someone else are
/// denied and write nothing.
#[test]
fn salesforce_process_rules_hold_against_a_fake_salesforce() {
    let writes = salesforce_writes_against_a_fake("process-rules");
    let paths: Vec<(&str, &str)> = writes
        .iter()
        .map(|(method, path, _)| (method.as_str(), path.as_str()))
        .collect();
    let mine = format!("{SALESFORCE_API}/sobjects/Opportunity/{OPPORTUNITY_MINE}");
    let task = format!("{SALESFORCE_API}/sobjects/Task");
    let approvals = format!("{SALESFORCE_API}/process/approvals");
    let feed = format!("{SALESFORCE_API}/chatter/feed-elements");
    assert_eq!(
        paths,
        vec![
            ("PATCH", mine.as_str()),
            ("POST", SALESFORCE_SOAP),
            ("POST", task.as_str()),
            ("POST", approvals.as_str()),
            ("POST", feed.as_str()),
        ],
        "only the allowed writes reach Salesforce"
    );
    let bodies: Vec<&serde_json::Value> = writes.iter().map(|(_, _, body)| body).collect();
    assert_eq!(
        bodies.first().copied(),
        Some(&serde_json::json!({ "OwnerId": "00G000000000001EAA" }))
    );
    let envelope = bodies
        .get(1)
        .and_then(|body| body.as_str())
        .unwrap_or_default();
    assert!(
        envelope.contains(&format!("<urn:leadId>{LEAD_MINE}</urn:leadId>"))
            && envelope.contains("<urn:convertedStatus>Closed - Converted</urn:convertedStatus>")
            && envelope.contains("<urn:doNotCreateOpportunity>true</urn:doNotCreateOpportunity>"),
        "the conversion: {envelope}"
    );
    assert_eq!(
        bodies.get(2).and_then(|body| body.get("WhatId")),
        Some(&serde_json::json!(OPPORTUNITY_MINE))
    );
    assert_eq!(
        bodies.get(3).copied(),
        Some(
            &serde_json::json!({ "requests": [{ "actionType": "Approve", "contextId": WORK_ITEM, "comments": "Within policy" }] })
        )
    );
}

/// Publishes a copy of the package pointed at a fake Salesforce, runs
/// `tests/fake-host/<script>.ts` under its blueprint, and returns the writes that
/// reached the fake (every request but reads), in order.
fn salesforce_writes_against_a_fake(script: &str) -> Vec<(String, String, serde_json::Value)> {
    let fake = FakeHost::start(SALESFORCE_TOKEN, salesforce_answer);
    let home = tempfile::tempdir().expect("package store");
    put_secret(home.path(), "policy-token", SALESFORCE_TOKEN);
    let project = tempfile::tempdir().expect("package copy");
    copy_package(project.path(), "salesforce");
    let published = Command::new(env!("CARGO_BIN_EXE_submilli"))
        .args([
            "build",
            "publish-local",
            "-p",
            "@submilli/salesforce",
            "--config",
            &format!("orgUrl=http://127.0.0.1:{}", fake.port),
        ])
        .current_dir(project.path())
        .env("SUBMILLI_HOME", home.path())
        .env("SUBMILLI_TELEMETRY", "0")
        .output()
        .expect("publish the package copy");
    assert!(
        published.status.success(),
        "{}",
        String::from_utf8_lossy(&published.stderr)
    );

    let directory = repository_root().join("packages/salesforce/tests/fake-host");
    let source = directory.join(format!("{script}.ts"));
    let blueprint = directory.join(format!("{script}.yaml"));
    let run = submilli(
        &[
            "run",
            &source.to_string_lossy(),
            "--blueprint",
            &blueprint.to_string_lossy(),
        ],
        home.path(),
    );
    assert!(
        run.status.success(),
        "{}{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );

    let requests = fake.requests();
    assert!(
        requests.iter().all(|request| request.authorized),
        "every request carries the token: {requests:#?}"
    );
    requests
        .into_iter()
        .filter(|request| request.method != "GET")
        .map(|request| (request.method, request.path, request.body))
        .collect()
}

/// Copies a package's sources into `project`, alone in its manifest, so that
/// building it against a fake leaves the repository's `capabilities.yaml` alone.
fn copy_package(project: &Path, package: &str) {
    let source = repository_root().join("packages").join(package);
    let target = project.join("packages").join(package);
    for directory in ["src", "docs"] {
        std::fs::create_dir_all(target.join(directory)).expect("package directory");
        for entry in std::fs::read_dir(source.join(directory)).expect("package sources") {
            let path = entry.expect("source entry").path();
            let name = path.file_name().expect("a source file name");
            std::fs::copy(&path, target.join(directory).join(name)).expect("copied source");
        }
    }
    std::fs::write(
        project.join("submilli.toml"),
        format!(
            "[[package]]\nname = \"@submilli/{package}\"\nversion = \"0.1.0\"\n\
             description = \"@submilli/{package} against a fake service\"\nkeywords = [\"{package}\"]\n\
             path = \"packages/{package}\"\n"
        ),
    )
    .expect("manifest");
}

/// A fake Salesforce: two opportunities on one account, one owned by the
/// caller's user and one by another user (a third id does not exist), two leads
/// owned likewise, and a discount approval of the caller's deal. Writes succeed.
fn salesforce_answer(
    method: &str,
    path: &str,
    query: &str,
) -> (&'static str, Option<serde_json::Value>) {
    use serde_json::json;
    let opportunity = |id: &str, owner: &str| {
        json!({ "attributes": { "type": "Opportunity", "url": format!("{SALESFORCE_API}/sobjects/Opportunity/{id}") },
            "Id": id, "Name": "Acme pilot", "OwnerId": owner, "AccountId": "001000000000001AAA",
            "StageName": "Prospecting", "Amount": 1200 })
    };
    let lead = |id: &str, owner: &str| json!({ "attributes": { "type": "Lead" }, "Id": id, "OwnerId": owner });
    let mine = opportunity(OPPORTUNITY_MINE, SALESFORCE_ME);
    let theirs = opportunity(OPPORTUNITY_THEIRS, SALESFORCE_OTHER);
    let not_found = || json!([{ "errorCode": "NOT_FOUND", "message": "The requested resource does not exist" }]);
    if method == "POST" && path == SALESFORCE_SOAP {
        let result = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><soapenv:Envelope xmlns:soapenv=\"http://schemas.xmlsoap.org/soap/envelope/\">\
             <soapenv:Body><convertLeadResponse><result><accountId>001000000000001AAA</accountId>\
             <contactId>003000000000001AAA</contactId><leadId>{LEAD_MINE}</leadId>\
             <opportunityId xsi:nil=\"true\"/><success>true</success></result></convertLeadResponse>\
             </soapenv:Body></soapenv:Envelope>"
        );
        return ("200 OK", Some(json!(result)));
    }
    let Some(rest) = path.strip_prefix(SALESFORCE_API) else {
        return ("404 Not Found", Some(not_found()));
    };
    let record = rest.strip_prefix("/sobjects/Opportunity/");
    match (method, record) {
        ("GET", Some(id)) if id == OPPORTUNITY_MINE => ("200 OK", Some(mine)),
        ("GET", Some(id)) if id == OPPORTUNITY_THEIRS => ("200 OK", Some(theirs)),
        ("PATCH" | "DELETE", Some(id)) if id == OPPORTUNITY_MINE || id == OPPORTUNITY_THEIRS => {
            ("204 No Content", None)
        }
        ("POST", None) if rest == "/sobjects/Task" => (
            "201 Created",
            Some(json!({ "id": "00T000000000001AAA", "success": true, "errors": [] })),
        ),
        ("POST", None) if rest == "/process/approvals" => (
            "200 OK",
            Some(
                json!([{ "success": true, "errors": null, "instanceId": "04g000000000001AAA",
                "instanceStatus": "Approved", "entityId": OPPORTUNITY_MINE, "newWorkitemIds": [], "actorIds": [] }]),
            ),
        ),
        ("POST", None) if rest == "/chatter/feed-elements" => {
            ("201 Created", Some(json!({ "id": "0D5000000000001AAA" })))
        }
        ("GET", None) if rest == "/query/" => {
            let soql = percent_decode(query.strip_prefix("q=").unwrap_or_default());
            let records = if soql.contains("FROM LeadStatus") {
                vec![
                    json!({ "attributes": { "type": "LeadStatus" }, "MasterLabel": "Closed - Converted" }),
                ]
            } else if soql.contains(WORK_ITEM) {
                vec![
                    json!({ "attributes": { "type": "ProcessInstanceWorkitem" }, "Id": WORK_ITEM,
                    "ProcessInstance": { "TargetObjectId": OPPORTUNITY_MINE,
                        "TargetObject": { "attributes": { "type": "Name" }, "Type": "Opportunity" },
                        "ProcessDefinition": { "DeveloperName": "Discount_Approval" } } }),
                ]
            } else if soql.contains(LEAD_MINE) {
                vec![lead(LEAD_MINE, SALESFORCE_ME)]
            } else if soql.contains(LEAD_THEIRS) {
                vec![lead(LEAD_THEIRS, SALESFORCE_OTHER)]
            } else if soql.contains(OPPORTUNITY_MINE) {
                vec![mine]
            } else if soql.contains(OPPORTUNITY_THEIRS) {
                vec![theirs]
            } else {
                Vec::new()
            };
            let size = records.len();
            (
                "200 OK",
                Some(json!({ "totalSize": size, "done": true, "records": records })),
            )
        }
        _ => ("404 Not Found", Some(not_found())),
    }
}

/// Decodes `%XX` escapes; the package encodes its query strings with `encodeComponent`.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let escaped = bytes
            .get(index + 1..index + 3)
            .filter(|_| bytes[index] == b'%')
            .and_then(|hex| u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok());
        if let Some(byte) = escaped {
            out.push(byte);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

const ZENDESK_TOKEN: &str = "fake-zendesk-token";

/// Every ticket check's facts (group, organization, brand, assignee, requester,
/// status) and a user's role come from Zendesk, so an offline script never
/// reaches them. `tests/fake-host/ticket-rules.ts` runs as `main` against a fake
/// Zendesk: group 100's ticket is read, updated, noted, reassigned within the
/// group and given a macro; group 200's ticket, a missing one, a priority
/// change, a public reply, a macro that replies or moves the ticket, and a move
/// to another group are denied and write nothing. Every write carries
/// `safe_update` with the `updated_at` read for its check, and a conflict comes
/// back as a retryable `UpdateConflict`.
#[test]
fn zendesk_ticket_rules_hold_against_a_fake_zendesk() {
    let fake = FakeHost::start(ZENDESK_TOKEN, zendesk_answer);
    let home = tempfile::tempdir().expect("package store");
    put_secret(home.path(), "policy-token", ZENDESK_TOKEN);
    let project = tempfile::tempdir().expect("package copy");
    copy_package(project.path(), "zendesk");
    let published = Command::new(env!("CARGO_BIN_EXE_submilli"))
        .args([
            "build",
            "publish-local",
            "-p",
            "@submilli/zendesk",
            "--config",
            &format!("accountUrl=http://127.0.0.1:{}", fake.port),
        ])
        .current_dir(project.path())
        .env("SUBMILLI_HOME", home.path())
        .env("SUBMILLI_TELEMETRY", "0")
        .output()
        .expect("publish the package copy");
    assert!(
        published.status.success(),
        "{}",
        String::from_utf8_lossy(&published.stderr)
    );
    let directory = repository_root().join("packages/zendesk/tests/fake-host");
    let run = submilli(
        &[
            "run",
            &directory.join("ticket-rules.ts").to_string_lossy(),
            "--blueprint",
            &directory.join("ticket-rules.yaml").to_string_lossy(),
        ],
        home.path(),
    );
    assert!(
        run.status.success(),
        "{}{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    let requests = fake.requests();
    assert!(
        requests.iter().all(|request| request.authorized),
        "every request carries the token: {requests:#?}"
    );
    let writes: Vec<(String, String, serde_json::Value)> = requests
        .into_iter()
        .filter(|request| request.method != "GET")
        .map(|request| (request.method, request.path, request.body))
        .collect();
    let stamped = |mut ticket: serde_json::Value| {
        ticket["safe_update"] = serde_json::json!(true);
        ticket["updated_stamp"] = serde_json::json!(ZENDESK_UPDATED);
        serde_json::json!({ "ticket": ticket })
    };
    let first = "/api/v2/tickets/1.json".to_string();
    assert_eq!(
        writes,
        vec![
            (
                "PUT".to_string(),
                first.clone(),
                stamped(serde_json::json!({ "status": "pending", "tags": ["vip"] }))
            ),
            (
                "PUT".to_string(),
                first.clone(),
                stamped(
                    serde_json::json!({ "comment": { "public": false, "body": "Checked the logs" } })
                )
            ),
            (
                "PUT".to_string(),
                first.clone(),
                stamped(serde_json::json!({ "assignee_id": 20 }))
            ),
            (
                "PUT".to_string(),
                first,
                stamped(
                    serde_json::json!({ "tags": ["vip", "billing"], "comment": { "public": false, "body": "Tagged for billing" } })
                )
            ),
            (
                "PUT".to_string(),
                "/api/v2/tickets/4.json".to_string(),
                stamped(serde_json::json!({ "status": "pending" }))
            ),
        ],
        "only the allowed writes reach Zendesk, each conditional on the ticket read"
    );
}

const ZENDESK_UPDATED: &str = "2026-11-20T10:00:00Z";

/// A fake Zendesk: tickets 1 and 4 in group 100 and ticket 2 in group 200
/// (ticket 3 does not exist; ticket 4 changed since it was read, so a write to it
/// conflicts), three macros on ticket 1 (51 tags it and adds a note, 50 solves
/// it with a public reply, 52 moves it to group 200), a customer (10) and an
/// agent (11).
fn zendesk_answer(
    method: &str,
    path: &str,
    _query: &str,
) -> (&'static str, Option<serde_json::Value>) {
    use serde_json::json;
    let ticket = |id: u64, group: u64| {
        json!({ "id": id, "subject": "Help", "status": "open", "group_id": group, "organization_id": 7,
            "brand_id": 1, "assignee_id": null, "requester_id": 10, "tags": ["vip"], "custom_fields": [],
            "updated_at": ZENDESK_UPDATED, "via": { "channel": "email" } })
    };
    let not_found = || json!({ "error": "RecordNotFound", "description": "Not found" });
    let macro_result = |changes: serde_json::Value| json!({ "result": { "ticket": changes } });
    let mut tagged = ticket(1, 100);
    tagged["tags"] = json!(["vip", "billing"]);
    tagged["comment"] = json!({ "body": "Tagged for billing", "public": "false" });
    let mut solved = ticket(1, 100);
    solved["status"] = json!("solved");
    solved["comment"] = json!({ "body": "We fixed it", "public": "true" });
    let mut moved = ticket(1, 200);
    moved["comment"] = json!(null);
    match (method, path) {
        ("GET" | "PUT", "/api/v2/tickets/1.json") => {
            ("200 OK", Some(json!({ "ticket": ticket(1, 100) })))
        }
        ("GET" | "PUT", "/api/v2/tickets/2.json") => {
            ("200 OK", Some(json!({ "ticket": ticket(2, 200) })))
        }
        ("GET", "/api/v2/tickets/4.json") => ("200 OK", Some(json!({ "ticket": ticket(4, 100) }))),
        ("PUT", "/api/v2/tickets/4.json") => (
            "409 Conflict",
            Some(
                json!({ "error": "UpdateConflict", "description": "Ticket changed since it was read" }),
            ),
        ),
        ("GET", "/api/v2/tickets/1/macros/51/apply.json") => ("200 OK", Some(macro_result(tagged))),
        ("GET", "/api/v2/tickets/1/macros/50/apply.json") => ("200 OK", Some(macro_result(solved))),
        ("GET", "/api/v2/tickets/1/macros/52/apply.json") => ("200 OK", Some(macro_result(moved))),
        ("GET", "/api/v2/users/10.json") => (
            "200 OK",
            Some(
                json!({ "user": { "id": 10, "name": "Dana", "role": "end-user", "organization_id": 7 } }),
            ),
        ),
        ("GET", "/api/v2/users/11.json") => (
            "200 OK",
            Some(
                json!({ "user": { "id": 11, "name": "Lee", "role": "agent", "organization_id": null } }),
            ),
        ),
        _ => ("404 Not Found", Some(not_found())),
    }
}

fn put_secret(home: &Path, name: &str, value: &str) {
    let mut put = Command::new(env!("CARGO_BIN_EXE_submilli"))
        .args(["secret", "put", name])
        .env("SUBMILLI_HOME", home)
        .env("SUBMILLI_TELEMETRY", "0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("put a secret");
    put.stdin
        .take()
        .expect("secret stdin")
        .write_all(value.as_bytes())
        .expect("write the secret");
    let result = put.wait_with_output().expect("secret put finished");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

/// Installs the packages that have required build settings with them, so that a
/// repository-wide build, which uses installed settings, can build them.
fn install_required_settings(home: &Path) {
    for (package, setting) in SETTINGS {
        let name = format!("@submilli/{package}");
        let published = submilli(
            &["build", "publish-local", "-p", &name, "--config", setting],
            home,
        );
        assert!(
            published.status.success(),
            "{}",
            String::from_utf8_lossy(&published.stderr)
        );
    }
}

/// Fails on any warning that names `check()`: a repeated read or a misplaced
/// `check()`, and a `check()` call with no `@capability`. These only warn, so a
/// package that regresses still builds.
#[test]
fn first_party_packages_build_without_check_warnings() {
    let home = tempfile::tempdir().expect("package store");
    install_required_settings(home.path());
    let checked = submilli(&["build", "check"], home.path());
    let stderr = String::from_utf8_lossy(&checked.stderr);
    assert!(checked.status.success(), "build check failed:\n{stderr}");
    let warnings: Vec<&str> = stderr
        .lines()
        .filter(|line| line.contains("warning") && line.contains("`check()`"))
        .collect();
    assert!(warnings.is_empty(), "{}", warnings.join("\n"));
}

/// The agent reads `docs/readme.md` while writing a program. It can run no
/// CLI command and cannot change its blueprint, so setup belongs in the root
/// `readme.md`, which people read. That includes the names of the secrets the
/// packages read: credentials are supplied internally, so the agent never
/// needs them.
#[test]
fn agent_docs_carry_no_setup() {
    const COMMANDS: &[&str] = &["submilli install", "submilli secret", "submilli blueprint"];
    // Blueprint YAML keys, starting a line after its indentation.
    const YAML_KEYS: &[&str] = &["secrets:", "harness:"];
    let packages = repository_root().join("packages");
    let package_dirs: Vec<PathBuf> = std::fs::read_dir(&packages)
        .expect("packages directory")
        .map(|entry| entry.expect("package entry").path())
        .collect();
    let secrets = secret_names(&package_dirs);
    assert!(
        !secrets.is_empty(),
        "no `secrets.get` calls found under packages/*/src"
    );
    let mut found = Vec::new();
    for package in &package_dirs {
        let readme = package.join("docs/readme.md");
        let Ok(text) = std::fs::read_to_string(&readme) else {
            continue;
        };
        for (number, line) in (1..).zip(text.lines()) {
            let trimmed = line.trim_start();
            if COMMANDS.iter().any(|command| line.contains(command))
                || YAML_KEYS.iter().any(|key| trimmed.starts_with(key))
                || secrets.iter().any(|secret| line.contains(secret.as_str()))
            {
                found.push(format!("{}:{number}: {line}", readme.display()));
            }
        }
    }
    assert!(
        found.is_empty(),
        "setup in agent docs:\n{}",
        found.join("\n")
    );
}

/// The names every package passes to `secrets.get("…")` in its `src/`.
fn secret_names(package_dirs: &[PathBuf]) -> Vec<String> {
    const CALL: &str = "secrets.get(\"";
    let mut names = Vec::new();
    for package in package_dirs {
        let Ok(sources) = std::fs::read_dir(package.join("src")) else {
            continue;
        };
        for source in sources {
            let Ok(text) = std::fs::read_to_string(source.expect("source entry").path()) else {
                continue;
            };
            for call in text.split(CALL).skip(1) {
                if let Some((name, _)) = call.split_once('"') {
                    names.push(name.to_string());
                }
            }
        }
    }
    names.sort();
    names.dedup();
    names
}

/// Every `<name>.ts` with a `<name>.yaml` beside it under the package's `tests/policy/`.
fn policy_scripts(package: &str) -> Vec<PathBuf> {
    let directory = repository_root()
        .join("packages")
        .join(package)
        .join("tests/policy");
    let mut scripts: Vec<PathBuf> = std::fs::read_dir(&directory)
        .unwrap_or_else(|error| panic!("reading {}: {error}", directory.display()))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "ts"))
        .filter(|path| path.with_extension("yaml").is_file())
        .collect();
    scripts.sort();
    scripts
}

fn submilli(args: &[&str], home: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_submilli"))
        .args(args)
        .current_dir(repository_root())
        .env("SUBMILLI_HOME", home)
        .env("SUBMILLI_TELEMETRY", "0")
        .output()
        .expect("invoke submilli")
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
