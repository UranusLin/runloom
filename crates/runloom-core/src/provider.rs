//! Explicit native-session resume plans. No credential reading or guessing by cwd.
use crate::{CommandSpec, Error, Work};
use serde::Serialize;
use std::{collections::BTreeMap, path::Path};
#[derive(Debug, Serialize)]
pub struct ProviderCapabilities {
    pub resume: bool,
    pub interactive: bool,
    pub headless: bool,
    pub checkpoint_pause: bool,
    pub cross_provider_conversion: bool,
}
pub fn resume_plan(
    work: &Work,
    executable: &str,
    prompt: Option<&str>,
    headless: bool,
) -> Result<CommandSpec, Error> {
    if !Path::new(executable).is_absolute()
        || !Path::new(&work.identity.directory).is_absolute()
        || !Path::new(&work.identity.config_root).is_absolute()
    {
        return Err(Error::Invalid("absolute provider paths"));
    }
    uuid::Uuid::parse_str(&work.identity.native_id)
        .map_err(|_| Error::Invalid("explicit native UUID"))?;
    let mut env = BTreeMap::new();
    let id = &work.identity.native_id;
    let mut args: Vec<String> = match work.identity.provider.as_str() {
        "codex" => {
            env.insert("CODEX_HOME".into(), work.identity.config_root.clone());
            if headless {
                vec![
                    "exec",
                    "-c",
                    "approval_policy=\"on-request\"",
                    "-c",
                    "sandbox_mode=\"workspace-write\"",
                    "resume",
                    "--json",
                    id,
                ]
                .into_iter()
                .map(str::to_owned)
                .collect()
            } else {
                vec![
                    "--ask-for-approval",
                    "on-request",
                    "--sandbox",
                    "workspace-write",
                    "resume",
                    id,
                ]
                .into_iter()
                .map(str::to_owned)
                .collect()
            }
        }
        "claude-code" => {
            // Setting the default root explicitly changes Claude's first-run/profile lookup.
            let default = std::env::var_os("HOME").map(|h| Path::new(&h).join(".claude"));
            if default.as_deref() != Some(Path::new(&work.identity.config_root)) {
                env.insert(
                    "CLAUDE_CONFIG_DIR".into(),
                    work.identity.config_root.clone(),
                );
            }
            let mut args = vec!["--permission-mode", "manual", "--resume", id]
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>();
            if headless {
                args.extend(["--print".into(), "--output-format".into(), "json".into()]);
            }
            args
        }
        _ => return Err(Error::Invalid("unsupported provider")),
    };
    if headless && prompt.is_none_or(|p| p.trim().is_empty()) {
        return Err(Error::Invalid("headless prompt"));
    }
    if let Some(prompt) = prompt {
        if prompt.len() > 16000 || prompt.contains('\0') {
            return Err(Error::Invalid("prompt"));
        }
        args.push("--".into());
        args.push(prompt.into());
    }
    Ok(CommandSpec {
        executable: executable.into(),
        args,
        directory: work.identity.directory.clone(),
        interactive: !headless,
        tty: !headless,
        env,
    })
}
impl ProviderCapabilities {
    pub fn for_provider(provider: &str) -> Self {
        let supported = matches!(provider, "codex" | "claude-code");
        Self {
            resume: supported,
            interactive: supported,
            headless: supported,
            checkpoint_pause: false,
            cross_provider_conversion: false,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::NewWork;
    #[test]
    fn plans_preserve_identity_without_bypass() {
        let mut w = Work {
            id: "work".into(),
            state: "registered".into(),
            identity: NewWork {
                workspace: "p".into(),
                host: "h".into(),
                provider: "codex".into(),
                config_root: "/fixture/profile".into(),
                native_id: uuid::Uuid::new_v4().to_string(),
                directory: "/fixture/repo".into(),
                title: "t".into(),
            },
        };
        for provider in ["codex", "claude-code"] {
            w.identity.provider = provider.into();
            for headless in [false, true] {
                let p =
                    resume_plan(&w, "/fixture/cli", Some("--literal prompt"), headless).unwrap();
                assert!(p.args.contains(&w.identity.native_id));
                assert_eq!(p.directory, w.identity.directory);
                assert!(!p.args.iter().any(|a| a.contains("bypass") || a == "--last"));
                assert_eq!(p.args.last().unwrap(), "--literal prompt");
            }
        }
        w.identity.native_id = "latest".into();
        assert!(resume_plan(&w, "/fixture/cli", Some("hi"), true).is_err());
    }
}
