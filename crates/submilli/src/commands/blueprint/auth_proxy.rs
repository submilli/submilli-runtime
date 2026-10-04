//! `submilli blueprint auth-proxy {add,list,remove}` — manage host-keyed
//! outbound-auth injection rules in a local `blueprint.yaml`'s `auth_proxy:`
//! block, so operators don't hand-write the `${secrets.X}` placeholder syntax.
//!
//! `add` takes the two common auth methods (`--bearer` / `--basic-*`) by **secret
//! name** — the credential value never touches the shell history or the YAML —
//! plus raw `--header` / `--query` injection.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{ArgGroup, Subcommand};
use submilli_blueprint::{AuthProxyRule, AuthSpec, BasicAuth, secret_refs};

use super::file::{blueprint_path, load, write};

#[derive(Subcommand)]
pub enum AuthProxyCmd {
    /// Add a host-keyed auth-injection rule to the `auth_proxy:` block.
    Add(AddArgs),
    /// List the blueprint's auth-proxy rules (never prints secret values).
    List(ListArgs),
    /// Remove the auth-proxy rule for a host.
    Remove(RemoveArgs),
}

pub fn execute(cmd: AuthProxyCmd) -> Result<ExitCode> {
    let result = match cmd {
        AuthProxyCmd::Add(args) => add(&args),
        AuthProxyCmd::List(args) => list(&args),
        AuthProxyCmd::Remove(args) => remove(&args),
    };
    match result {
        Ok(message) => {
            println!("{message}");
            Ok(ExitCode::SUCCESS)
        }
        Err(e) => {
            eprintln!("error: {e:#}");
            Ok(ExitCode::from(1))
        }
    }
}

/// At least one injection must be given; `--bearer` and `--basic-*` are mutually
/// exclusive (enforced by `conflicts_with`), and `--basic-username`/`--basic-password`
/// are required together.
#[derive(clap::Args)]
#[command(group = ArgGroup::new("injection").required(true)
    .args(["bearer", "basic_username", "headers", "query"]))]
pub struct AddArgs {
    /// Destination host to match exactly (e.g. `api.github.com`).
    #[arg(long)]
    host: String,
    /// Permit HTTP for this rule; the blueprint must separately allow_insecure_http.
    #[arg(long)]
    allow_insecure_http: bool,
    /// Bearer-token auth: names a declared secret. Sets `Authorization: Bearer <secret>`.
    #[arg(long, value_name = "SECRET_NAME", conflicts_with_all = ["basic_username", "basic_password"])]
    bearer: Option<String>,
    /// Basic-auth username (a literal, not a secret). Requires `--basic-password`.
    #[arg(long, value_name = "USERNAME", requires = "basic_password")]
    basic_username: Option<String>,
    /// Basic-auth password: names a declared secret. Requires `--basic-username`.
    #[arg(long, value_name = "SECRET_NAME", requires = "basic_username")]
    basic_password: Option<String>,
    /// Raw header injection, `Name=value` (repeatable). Value may use `${secrets.X}`.
    #[arg(long = "header", value_name = "NAME=VALUE")]
    headers: Vec<String>,
    /// Raw query-param injection, `key=value` (repeatable). Value may use `${secrets.X}`.
    #[arg(long = "query", value_name = "KEY=VALUE")]
    query: Vec<String>,
    /// Blueprint file to edit (default: ./blueprint.yaml).
    #[arg(long)]
    blueprint: Option<PathBuf>,
}

#[derive(clap::Args)]
pub struct ListArgs {
    /// Blueprint file to read (default: ./blueprint.yaml).
    #[arg(long)]
    blueprint: Option<PathBuf>,
}

#[derive(clap::Args)]
pub struct RemoveArgs {
    /// The rule's host to remove.
    #[arg(long)]
    host: String,
    /// Blueprint file to edit (default: ./blueprint.yaml).
    #[arg(long)]
    blueprint: Option<PathBuf>,
}

fn add(args: &AddArgs) -> Result<String> {
    let path = blueprint_path(&args.blueprint);
    let mut blueprint = load(&path)?;

    if blueprint.auth_proxy.iter().any(|r| r.host == args.host) {
        bail!(
            "blueprint '{}' already has an auth_proxy rule for host '{}'",
            blueprint.name,
            args.host
        );
    }

    let auth = build_auth(args);
    let headers = parse_pairs(&args.headers, "header")?;
    let query = parse_pairs(&args.query, "query")?;

    // Fail early with an actionable hint rather than the generic "invalid blueprint"
    // the re-parse below would surface. bearer / basic password name secrets directly;
    // header / query values may embed `${secrets.X}` refs.
    let referenced: Vec<&str> = auth
        .iter()
        .flat_map(auth_secret_names)
        .chain(
            headers
                .values()
                .chain(query.values())
                .flat_map(|v| secret_refs(v)),
        )
        .collect();
    for name in referenced {
        if !blueprint.secrets.contains_key(name) {
            bail!(
                "references secret '{name}', not declared in {}.\n  \
                 Declare it first, e.g.: submilli blueprint secret add {name} --store <KEY>",
                path.display()
            );
        }
    }

    let describe = describe(&auth, &headers, &query);
    blueprint.auth_proxy.push(AuthProxyRule {
        host: args.host.clone(),
        allow_insecure_http: args.allow_insecure_http,
        auth,
        headers,
        query,
    });
    write(&path, &blueprint)?;

    Ok(format!(
        "✓ added auth_proxy rule for host '{}' ({describe}) in {}",
        args.host,
        path.display()
    ))
}

fn list(args: &ListArgs) -> Result<String> {
    let path = blueprint_path(&args.blueprint);
    let blueprint = load(&path)?;
    if blueprint.auth_proxy.is_empty() {
        return Ok(format!("no auth_proxy rules in {}", path.display()));
    }
    let mut out = String::new();
    for rule in &blueprint.auth_proxy {
        out.push_str(&format!("{}\n", rule.host));
        if rule.allow_insecure_http {
            out.push_str("  allow_insecure_http: true (requires blueprint opt-in)\n");
        }
        if let Some(auth) = &rule.auth {
            if let Some(secret) = &auth.bearer {
                out.push_str(&format!("  auth: bearer ({secret})\n"));
            }
            if let Some(basic) = &auth.basic {
                out.push_str(&format!(
                    "  auth: basic (user={}, password={})\n",
                    basic.username, basic.password
                ));
            }
        }
        for name in rule.headers.keys() {
            out.push_str(&format!("  header: {name}\n"));
        }
        for name in rule.query.keys() {
            out.push_str(&format!("  query: {name}\n"));
        }
    }
    Ok(out.trim_end().to_string())
}

fn remove(args: &RemoveArgs) -> Result<String> {
    let path = blueprint_path(&args.blueprint);
    let mut blueprint = load(&path)?;
    let before = blueprint.auth_proxy.len();
    blueprint.auth_proxy.retain(|r| r.host != args.host);
    if blueprint.auth_proxy.len() == before {
        bail!(
            "no auth_proxy rule for host '{}' in {}",
            args.host,
            path.display()
        );
    }
    write(&path, &blueprint)?;
    Ok(format!(
        "✓ removed auth_proxy rule for host '{}' from {}",
        args.host,
        path.display()
    ))
}

/// The `auth:` block from the `--bearer` / `--basic-*` flags, if any.
fn build_auth(args: &AddArgs) -> Option<AuthSpec> {
    if let Some(secret) = &args.bearer {
        return Some(AuthSpec {
            bearer: Some(secret.clone()),
            basic: None,
        });
    }
    if let (Some(username), Some(password)) = (&args.basic_username, &args.basic_password) {
        return Some(AuthSpec {
            bearer: None,
            basic: Some(BasicAuth {
                username: username.clone(),
                password: password.clone(),
            }),
        });
    }
    None
}

/// The declared-secret names an `auth:` block references (username is a literal).
fn auth_secret_names(auth: &AuthSpec) -> Vec<&str> {
    auth.bearer
        .as_deref()
        .into_iter()
        .chain(auth.basic.as_ref().map(|b| b.password.as_str()))
        .collect()
}

/// Parse `Name=value` arguments into a map, splitting on the first `=`.
fn parse_pairs(raw: &[String], kind: &str) -> Result<BTreeMap<String, String>> {
    let mut pairs = BTreeMap::new();
    for entry in raw {
        let (name, value) = entry
            .split_once('=')
            .with_context(|| format!("{kind} '{entry}' must be in `Name=value` form"))?;
        let name = name.trim();
        if name.is_empty() {
            bail!("{kind} '{entry}' has an empty name");
        }
        pairs.insert(name.to_string(), value.to_string());
    }
    Ok(pairs)
}

fn describe(
    auth: &Option<AuthSpec>,
    headers: &BTreeMap<String, String>,
    query: &BTreeMap<String, String>,
) -> String {
    let mut parts = Vec::new();
    match auth {
        Some(a) if a.bearer.is_some() => parts.push("bearer auth".to_string()),
        Some(a) if a.basic.is_some() => parts.push("basic auth".to_string()),
        _ => {}
    }
    if !headers.is_empty() {
        parts.push(format!("{} header(s)", headers.len()));
    }
    if !query.is_empty() {
        parts.push(format!("{} query param(s)", query.len()));
    }
    parts.join(", ")
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::*;

    fn add_args(host: &str, path: &Path) -> AddArgs {
        AddArgs {
            host: host.into(),
            allow_insecure_http: false,
            bearer: None,
            basic_username: None,
            basic_password: None,
            headers: Vec::new(),
            query: Vec::new(),
            blueprint: Some(path.to_path_buf()),
        }
    }

    fn temp_blueprint(body: &str) -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("blueprint.yaml");
        fs::write(&path, body).unwrap();
        (tmp, path)
    }

    fn reload(path: &Path) -> submilli_blueprint::Blueprint {
        submilli_blueprint::parse(&fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn insecure_http_rule_opt_in_does_not_enable_blueprint() {
        let (_tmp, path) = temp_blueprint("name: t\n");
        let mut args = add_args("localhost", &path);
        args.headers.push("X-Test=value".into());
        args.allow_insecure_http = true;
        add(&args).unwrap();
        let blueprint = reload(&path);
        assert!(blueprint.auth_proxy[0].allow_insecure_http);
        assert!(!blueprint.allow_insecure_http);
        assert!(
            list(&ListArgs {
                blueprint: Some(path)
            })
            .unwrap()
            .contains("allow_insecure_http: true")
        );
    }

    #[test]
    fn bearer_writes_auth_bearer() {
        let (_tmp, path) = temp_blueprint("name: t\nsecrets:\n  GH: { store: GH }\n");
        let mut a = add_args("api.github.com", &path);
        a.bearer = Some("GH".into());
        add(&a).unwrap();
        let rule = &reload(&path).auth_proxy[0];
        assert_eq!(rule.host, "api.github.com");
        assert_eq!(rule.auth.as_ref().unwrap().bearer.as_deref(), Some("GH"));
    }

    #[test]
    fn basic_writes_auth_basic() {
        let (_tmp, path) = temp_blueprint("name: t\nsecrets:\n  P: { store: P }\n");
        let mut a = add_args("api.example.com", &path);
        a.basic_username = Some("alice".into());
        a.basic_password = Some("P".into());
        add(&a).unwrap();
        let basic = reload(&path).auth_proxy[0]
            .auth
            .clone()
            .unwrap()
            .basic
            .unwrap();
        assert_eq!(basic.username, "alice");
        assert_eq!(basic.password, "P");
    }

    #[test]
    fn header_and_query_injection() {
        let (_tmp, path) = temp_blueprint("name: t\nsecrets:\n  K: { store: K }\n");
        let mut a = add_args("api.example.com", &path);
        a.headers = vec!["X-Api-Key=${secrets.K}".into()];
        a.query = vec!["appid=${secrets.K}".into()];
        add(&a).unwrap();
        let rule = &reload(&path).auth_proxy[0];
        assert_eq!(rule.headers["X-Api-Key"], "${secrets.K}");
        assert_eq!(rule.query["appid"], "${secrets.K}");
    }

    #[test]
    fn undeclared_secret_is_rejected_with_hint() {
        let (_tmp, path) = temp_blueprint("name: t\n");
        let mut a = add_args("api.github.com", &path);
        a.bearer = Some("MISSING".into());
        let err = add(&a).unwrap_err();
        assert!(err.to_string().contains("not declared"), "{err}");
    }

    #[test]
    fn duplicate_host_is_rejected() {
        let (_tmp, path) = temp_blueprint("name: t\nsecrets:\n  GH: { store: GH }\n");
        let mut a = add_args("api.github.com", &path);
        a.bearer = Some("GH".into());
        add(&a).unwrap();
        let mut dup = add_args("api.github.com", &path);
        dup.bearer = Some("GH".into());
        let err = add(&dup).unwrap_err();
        assert!(err.to_string().contains("already has"), "{err}");
    }

    #[test]
    fn list_prints_rules() {
        let (_tmp, path) = temp_blueprint("name: t\nsecrets:\n  GH: { store: GH }\n");
        let mut a = add_args("api.github.com", &path);
        a.bearer = Some("GH".into());
        add(&a).unwrap();
        let out = list(&ListArgs {
            blueprint: Some(path.clone()),
        })
        .unwrap();
        assert!(out.contains("api.github.com"));
        assert!(out.contains("bearer"));
    }

    #[test]
    fn remove_drops_the_rule() {
        let (_tmp, path) = temp_blueprint("name: t\nsecrets:\n  GH: { store: GH }\n");
        let mut a = add_args("api.github.com", &path);
        a.bearer = Some("GH".into());
        add(&a).unwrap();
        remove(&RemoveArgs {
            host: "api.github.com".into(),
            blueprint: Some(path.clone()),
        })
        .unwrap();
        assert!(reload(&path).auth_proxy.is_empty());
    }

    #[test]
    fn remove_unknown_host_is_an_error() {
        let (_tmp, path) = temp_blueprint("name: t\n");
        let err = remove(&RemoveArgs {
            host: "nope".into(),
            blueprint: Some(path.clone()),
        })
        .unwrap_err();
        assert!(err.to_string().contains("no auth_proxy rule"), "{err}");
    }
}
