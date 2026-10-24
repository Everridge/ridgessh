//! Line-level tokenizer. Every line keeps its exact original text so the file can be
//! reproduced byte for byte; the parsed fields are only an interpretation of it.

/// How a line is understood. `raw` on [`Line`] is always the source of truth.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineKind {
    Blank,
    Comment,
    Directive {
        /// Keyword as written (`HostName`, `hostname`, ...).
        key: String,
        /// Everything after the keyword and its separator, trailing whitespace removed.
        value: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// Original text without the line terminator.
    pub raw: String,
    pub kind: LineKind,
}

impl Line {
    pub fn parse(raw: &str) -> Self {
        Self {
            raw: raw.to_string(),
            kind: classify(raw),
        }
    }

    /// Lowercased keyword for directives.
    pub fn keyword(&self) -> Option<String> {
        match &self.kind {
            LineKind::Directive { key, .. } => Some(key.to_ascii_lowercase()),
            _ => None,
        }
    }

    pub fn value(&self) -> Option<&str> {
        match &self.kind {
            LineKind::Directive { value, .. } => Some(value),
            _ => None,
        }
    }
}

fn classify(raw: &str) -> LineKind {
    let s = raw.trim_start();
    if s.is_empty() {
        return LineKind::Blank;
    }
    if s.starts_with('#') {
        return LineKind::Comment;
    }
    // OpenSSH: keyword, then whitespace and/or a single '=', then the arguments.
    let key_end = s
        .find(|c: char| c.is_whitespace() || c == '=')
        .unwrap_or(s.len());
    let key = &s[..key_end];
    let mut rest = s[key_end..].trim_start();
    if let Some(r) = rest.strip_prefix('=') {
        rest = r.trim_start();
    }
    LineKind::Directive {
        key: key.to_string(),
        value: rest.trim_end().to_string(),
    }
}

/// Splits an argument list the way OpenSSH does: whitespace separated, `"..."` groups.
pub fn split_args(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut has_token = false;
    for c in value.chars() {
        match c {
            '"' => {
                in_quotes = !in_quotes;
                has_token = true;
            }
            c if c.is_whitespace() && !in_quotes => {
                if has_token {
                    out.push(std::mem::take(&mut cur));
                    has_token = false;
                }
            }
            c => {
                cur.push(c);
                has_token = true;
            }
        }
    }
    if has_token {
        out.push(cur);
    }
    out
}

/// Removes one pair of surrounding double quotes, if present.
pub fn unquote(value: &str) -> String {
    let v = value.trim();
    if v.len() >= 2 && v.starts_with('"') && v.ends_with('"') {
        v[1..v.len() - 1].to_string()
    } else {
        v.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn directive(raw: &str) -> (String, String) {
        match Line::parse(raw).kind {
            LineKind::Directive { key, value } => (key, value),
            k => panic!("not a directive: {k:?}"),
        }
    }

    #[test]
    fn classifies_lines() {
        assert_eq!(Line::parse("").kind, LineKind::Blank);
        assert_eq!(Line::parse("   \t").kind, LineKind::Blank);
        assert_eq!(Line::parse("  # note").kind, LineKind::Comment);
        assert_eq!(directive("Host web"), ("Host".into(), "web".into()));
        assert_eq!(
            directive("    HostName=10.0.0.1"),
            ("HostName".into(), "10.0.0.1".into())
        );
        assert_eq!(directive("Port = 2222  "), ("Port".into(), "2222".into()));
        assert_eq!(
            directive("\tUser\tdeploy"),
            ("User".into(), "deploy".into())
        );
        assert_eq!(
            directive("ForwardAgent"),
            ("ForwardAgent".into(), "".into())
        );
    }

    #[test]
    fn splits_quoted_args() {
        assert_eq!(split_args(r#"a  "b c" d"#), ["a", "b c", "d"]);
        assert_eq!(
            split_args(r#""C:\Users\me\.ssh\id rsa""#),
            [r"C:\Users\me\.ssh\id rsa"]
        );
        assert_eq!(split_args(r#""""#), [""]);
        assert!(split_args("   ").is_empty());
    }

    #[test]
    fn unquotes() {
        assert_eq!(unquote(r#""a b""#), "a b");
        assert_eq!(unquote("plain"), "plain");
    }
}
