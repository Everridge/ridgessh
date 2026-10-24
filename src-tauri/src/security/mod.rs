//! Input validation for values that end up as process arguments or paths.

use crate::error::{AppError, AppResult};

/// Validates an SSH `Host` alias before it is passed to `ssh`/`scp`.
///
/// Rejects anything that could be parsed as an option (leading `-`), wildcard/negation
/// patterns, pattern lists, whitespace, quotes and control characters.
pub fn validate_alias(alias: &str) -> AppResult<()> {
    let invalid = alias.is_empty()
        || alias.len() > 255
        || alias.starts_with('-')
        || alias.chars().any(|c| {
            c.is_whitespace() || c.is_control() || matches!(c, '*' | '?' | '!' | '"' | '\'' | ',')
        });
    if invalid {
        return Err(AppError::Validation(format!(
            "invalid host alias: {alias:?}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_normal_aliases() {
        for a in ["prod-db", "billing_server", "web01.example.com", "10.0.0.1"] {
            assert!(validate_alias(a).is_ok(), "{a}");
        }
    }

    #[test]
    fn rejects_dangerous_aliases() {
        for a in [
            "",
            "-oProxyCommand=calc",
            "a b",
            "web*",
            "!neg",
            "x\ny",
            "a,b",
            "\"q\"",
        ] {
            assert!(validate_alias(a).is_err(), "{a:?}");
        }
    }
}
