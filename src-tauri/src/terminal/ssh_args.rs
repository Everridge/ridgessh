//! Builds argument vectors for the system OpenSSH client. Arguments are passed as a vector
//! (never through a shell), and every value was validated when it entered the database.

use std::path::PathBuf;

use crate::db::hosts::Host;
use crate::error::{AppError, AppResult};
use crate::security::{validate_alias, validate_user};
use crate::ssh_config::lexer::quote_arg;

/// Locates `ssh` (or `scp`). The system OpenSSH is preferred (Windows' inbox copy over e.g.
/// Git's, `/usr/bin` on macOS over Homebrew's), so the same agent and config handling applies
/// as in a normal terminal.
pub fn find_tool(name: &str) -> AppResult<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let system = PathBuf::from("/usr/bin").join(name);
        if system.is_file() {
            return Ok(system);
        }
    }
    #[cfg(windows)]
    {
        if let Some(root) = std::env::var_os("SystemRoot") {
            let inbox = PathBuf::from(root)
                .join("System32")
                .join("OpenSSH")
                .join(format!("{name}.exe"));
            if inbox.is_file() {
                return Ok(inbox);
            }
        }
    }
    which::which(name).map_err(|_| {
        let hint = if cfg!(windows) {
            " Install the OpenSSH client (Settings → System → Optional features)."
        } else {
            " Install the OpenSSH client."
        };
        AppError::NotFound(format!("{name} was not found.{hint}"))
    })
}

/// Destination plus `-o` overrides for `host`.
///
/// Hosts that match their `~/.ssh/config` block connect by alias only, so the config applies
/// exactly as in a terminal. Hosts that are not in the config, or have edits not yet written to
/// it, get their RidgeSSH values as `-o` options (command-line options take precedence over
/// the config file in OpenSSH).
///
/// With `user` the destination is `user@alias`, exactly what one would type for a per-user
/// `Match originalhost alias user name` block, so OpenSSH applies that block (its key).
/// No `--` is needed: the alias and user are validated never to start with `-`.
pub fn target_args(host: &Host, user: Option<&str>) -> AppResult<Vec<String>> {
    let (mut args, destination) = host_options(host, user)?;
    args.push(destination);
    Ok(args)
}

/// The `-o` overrides for `host` (see [`target_args`]) and the destination alias, separately,
/// for tools that put more arguments between them (scp, remote commands).
pub fn host_options(host: &Host, user: Option<&str>) -> AppResult<(Vec<String>, String)> {
    let alias = host
        .config_snapshot
        .as_ref()
        .map(|s| s.alias.as_str())
        .unwrap_or(&host.alias);
    validate_alias(alias)?;
    let destination = match user {
        Some(u) => {
            validate_user(u)?;
            format!("{u}@{alias}")
        }
        None => alias.to_string(),
    };

    let mut args = Vec::new();
    if host.config_snapshot.is_none() || host.config_dirty {
        let mut opt = |key: &str, value: String| {
            args.push("-o".to_string());
            args.push(format!("{key}={value}"));
        };
        // Single-value fields hold a plain value and get quoted for OpenSSH's tokenizer.
        let single = [
            ("HostName", host.hostname.clone()),
            // `-o User` would win over `user@`, so it is left out when connecting as a user.
            ("User", host.user.clone().filter(|_| user.is_none())),
            ("Port", host.port.map(|p| p.to_string())),
            ("IdentityFile", host.identity_file.clone()),
            ("ProxyJump", host.proxy_jump.clone()),
        ];
        for (key, value) in single {
            if let Some(v) = value {
                opt(key, quote_arg(&v));
            }
        }
        // Other options are stored in config syntax already (possibly several arguments).
        for d in &host.extra_options {
            opt(&d.key, d.value.clone());
        }
    }
    Ok((args, destination))
}

/// Quotes one word for a POSIX shell on the remote side: `it's` → `'it'\''s'`.
pub fn posix_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::hosts::{Connection, Directive};

    pub(super) fn host_for_tests() -> Host {
        host()
    }

    fn host() -> Host {
        Host {
            id: "1".into(),
            alias: "web".into(),
            hostname: Some("10.0.0.1".into()),
            user: Some("deploy".into()),
            port: Some(2222),
            identity_file: Some("C:\\keys\\id ed".into()),
            proxy_jump: None,
            extra_options: vec![Directive {
                key: "LocalForward".into(),
                value: "8080 localhost:80".into(),
            }],
            group_id: None,
            description: String::new(),
            notes: String::new(),
            color: None,
            favorite: false,
            sync_state: "db_only".into(),
            config_dirty: false,
            config_snapshot: None,
            config_source: None,
            config_fingerprint: None,
            last_seen_in_config_at: None,
            removed_from_config_at: None,
            last_connected_at: None,
            deleted_at: None,
            created_at: String::new(),
            updated_at: String::new(),
            tags: vec![],
            fields: vec![],
        }
    }

    #[test]
    fn db_only_host_passes_all_values_as_options() {
        assert_eq!(
            target_args(&host(), None).unwrap(),
            [
                "-o",
                "HostName=10.0.0.1",
                "-o",
                "User=deploy",
                "-o",
                "Port=2222",
                "-o",
                "IdentityFile=\"C:\\keys\\id ed\"",
                "-o",
                "LocalForward=8080 localhost:80",
                "web"
            ]
        );
    }

    #[test]
    fn synced_host_connects_by_alias_only() {
        let mut h = host();
        h.config_snapshot = Some(Connection {
            alias: "web".into(),
            ..Default::default()
        });
        assert_eq!(target_args(&h, None).unwrap(), ["web"]);
        assert_eq!(target_args(&h, Some("tom")).unwrap(), ["tom@web"]);
        assert!(target_args(&h, Some("-lroot")).is_err());
        assert!(target_args(&h, Some("a@b")).is_err());
    }

    #[test]
    fn user_replaces_the_user_override() {
        let args = target_args(&host(), Some("tom")).unwrap();
        assert!(!args.iter().any(|a| a.starts_with("User=")));
        assert_eq!(args.last().unwrap(), "tom@web");
    }

    #[test]
    fn renamed_dirty_host_uses_config_alias_with_overrides() {
        let mut h = host();
        h.alias = "web-renamed".into();
        h.config_dirty = true;
        h.config_snapshot = Some(Connection {
            alias: "web".into(),
            ..Default::default()
        });
        let args = target_args(&h, None).unwrap();
        assert_eq!(args.last().unwrap(), "web");
        assert!(args.contains(&"Port=2222".to_string()));
    }

    #[test]
    fn refuses_option_like_destination() {
        let mut h = host();
        h.alias = "-oProxyCommand=calc".into();
        assert!(target_args(&h, None).is_err());
    }
}

/// Feeds the generated arguments to the real `ssh -G` (prints the resolved config without
/// connecting) to prove OpenSSH accepts them. Uses a made-up host so nothing real is touched.
#[cfg(test)]
mod real_ssh {
    use super::*;
    use crate::db::hosts::Directive;

    #[test]
    fn real_ssh_accepts_generated_options() {
        let Ok(ssh) = find_tool("ssh") else {
            eprintln!("ssh not installed; skipping");
            return;
        };
        let mut h = tests_support::host();
        h.alias = "ridgessh-selftest".into();
        h.hostname = Some("ridgessh-selftest.invalid".into());
        h.extra_options = vec![Directive {
            key: "LocalForward".into(),
            value: "15433 localhost:5432".into(),
        }];
        let mut args = vec!["-G".to_string()];
        args.extend(target_args(&h, None).unwrap());
        let out = std::process::Command::new(ssh)
            .args(&args)
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout).to_lowercase();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(text.contains("hostname ridgessh-selftest.invalid"));
        assert!(text.contains("port 2222"));
        assert!(text.contains("user deploy"));
        // Only the relevant line is printed: the full `-G` dump includes the user's global config.
        let id_line = text
            .lines()
            .find(|l| l.contains("id ed"))
            .unwrap_or("<none>");
        assert_eq!(id_line, r"identityfile c:\keys\id ed");
        assert!(
            text.contains("localforward 15433 localhost:5432")
                || text.contains("localforward 15433 [localhost]:5432")
        );
    }
}

#[cfg(test)]
pub(crate) mod tests_support {
    pub fn host() -> crate::db::hosts::Host {
        super::tests::host_for_tests()
    }
}

#[cfg(test)]
mod real_ssh_paths {
    use super::*;

    #[test]
    fn real_ssh_keeps_unc_and_quoted_paths() {
        let Ok(ssh) = find_tool("ssh") else { return };
        let mut h = tests_support::host();
        h.alias = "ridgessh-selftest".into();
        h.extra_options.clear();
        h.identity_file = Some(r#"\\server\share\my "key""#.into());
        let mut args = vec!["-G".to_string()];
        args.extend(target_args(&h, None).unwrap());
        let out = std::process::Command::new(ssh)
            .args(&args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let text = String::from_utf8_lossy(&out.stdout).to_lowercase();
        assert!(
            text.contains(r#"identityfile \\server\share\my "key""#),
            "{text}"
        );
    }
}
