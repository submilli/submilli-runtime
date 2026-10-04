//! Local authoring of the blueprint's Git configuration.
use super::file::{blueprint_path, load, write};
use anyhow::Result;
use clap::Subcommand;
use std::path::PathBuf;
use std::process::ExitCode;
use submilli_blueprint::{GitConfig, GitIdentity};

#[derive(Subcommand)]
pub enum GitCmd {
    /// Enable or update Git. Values may contain declared ${vars.NAME} references.
    Set(SetArgs),
    /// Show Git templates and token declaration, never secret values.
    Show(FileArgs),
    /// Disable Git without removing secrets, variables, or permissions.
    Remove(FileArgs),
}

#[derive(clap::Args)]
pub struct FileArgs {
    #[arg(long)]
    blueprint: Option<PathBuf>,
}

#[derive(clap::Args)]
pub struct SetArgs {
    #[arg(long)]
    name: String,
    #[arg(long)]
    email: String,
    #[arg(long, conflicts_with = "clear_username")]
    username: Option<String>,
    #[arg(long)]
    clear_username: bool,
    #[arg(long)]
    blueprint: Option<PathBuf>,
}

pub fn execute(cmd: GitCmd) -> Result<ExitCode> {
    match run(cmd) {
        Ok(message) => {
            println!("{message}");
            Ok(ExitCode::SUCCESS)
        }
        Err(error) => {
            eprintln!("error: {error:#}");
            Ok(ExitCode::from(1))
        }
    }
}

fn run(cmd: GitCmd) -> Result<String> {
    let path = blueprint_path(match &cmd {
        GitCmd::Set(args) => &args.blueprint,
        GitCmd::Show(args) | GitCmd::Remove(args) => &args.blueprint,
    });
    let mut blueprint = load(&path)?;
    match cmd {
        GitCmd::Set(args) => {
            let previous = blueprint.git.as_ref().and_then(|git| git.username.clone());
            blueprint.git = Some(GitConfig {
                identity: GitIdentity {
                    name: args.name,
                    email: args.email,
                },
                username: if args.clear_username {
                    None
                } else {
                    args.username.or(previous)
                },
            });
            write(&path, &blueprint)?;
            Ok(format!(
                "Configured Git in {}. Configure grants with `submilli blueprint capability add`.",
                path.display()
            ))
        }
        GitCmd::Show(_) => {
            let Some(git) = blueprint.git else {
                return Ok("Git is disabled.".into());
            };
            Ok(format!(
                "name: {}\nemail: {}\nusername: {}\nGIT_TOKEN declared: {}",
                git.identity.name,
                git.identity.email,
                git.username.as_deref().unwrap_or("(not set)"),
                blueprint.secrets.contains_key("GIT_TOKEN")
            ))
        }
        GitCmd::Remove(_) => {
            blueprint.git = None;
            write(&path, &blueprint)?;
            Ok(format!("Disabled Git in {}", path.display()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn configure_update_and_remove_preserve_other_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("blueprint.yaml");
        std::fs::write(&path, "name: test\nsecrets:\n  GIT_TOKEN: {store: TOKEN}\n").unwrap();
        for username in [Some("git".into()), None] {
            run(GitCmd::Set(SetArgs {
                name: "Agent".into(),
                email: "agent@example.com".into(),
                username,
                clear_username: false,
                blueprint: Some(path.clone()),
            }))
            .unwrap();
        }
        assert_eq!(
            load(&path).unwrap().git.unwrap().username.as_deref(),
            Some("git")
        );
        let show = || {
            run(GitCmd::Show(FileArgs {
                blueprint: Some(path.clone()),
            }))
            .unwrap()
        };
        assert_eq!(
            show(),
            "name: Agent\nemail: agent@example.com\nusername: git\nGIT_TOKEN declared: true"
        );
        let before = std::fs::read(&path).unwrap();
        assert!(
            run(GitCmd::Set(SetArgs {
                name: "${vars.missing}".into(),
                email: "a@b".into(),
                username: None,
                clear_username: false,
                blueprint: Some(path.clone())
            }))
            .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
        run(GitCmd::Remove(FileArgs {
            blueprint: Some(path.clone()),
        }))
        .unwrap();
        let bp = load(&path).unwrap();
        assert!(bp.git.is_none());
        assert_eq!(show(), "Git is disabled.");
        assert!(bp.secrets.contains_key("GIT_TOKEN"));
    }
}
