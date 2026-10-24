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

    /// Leading whitespace.
    pub fn indent(&self) -> &str {
        &self.raw[..self.raw.len() - self.raw.trim_start().len()]
    }

    /// The same directive with a new value, keeping indentation, keyword spelling, separator
    /// (`Key value` vs `Key=value`) and trailing whitespace. The whole old value is replaced.
    pub fn with_value(&self, new_value: &str) -> String {
        let LineKind::Directive { key, value } = &self.kind else {
            return self.raw.clone();
        };
        let body = self.raw.trim_end();
        let trailing = &self.raw[body.len()..];
        let prefix = if value.is_empty() {
            format!("{body} ")
        } else {
            // `value` is the trimmed remainder of the line, so it starts where the prefix ends.
            debug_assert!(body[self.indent().len()..].starts_with(key.as_str()));
            body[..body.len() - value.len()].to_string()
        };
        format!("{prefix}{new_value}{trailing}")
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

/// Splits an argument list exactly like OpenSSH's `argv_split` (as used by readconf):
/// whitespace separated, `"..."` / `'...'` groups, `\\` `\"` `\'` escapes (and `\ ` outside
/// quotes), and a `#` at the start of a token ends the line.
pub fn split_args(value: &str) -> Vec<String> {
    let chars: Vec<char> = value.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == ' ' || chars[i] == '\t' {
            i += 1;
            continue;
        }
        if chars[i] == '#' {
            break;
        }
        let mut quote: Option<char> = None;
        let mut arg = String::new();
        while i < chars.len() {
            let c = chars[i];
            match (c, quote) {
                ('\\', _) => {
                    let next = chars.get(i + 1).copied();
                    let escape = matches!(next, Some('\'' | '"' | '\\'))
                        || (quote.is_none() && next == Some(' '));
                    if escape {
                        i += 1;
                        arg.push(chars[i]);
                    } else {
                        arg.push('\\');
                    }
                }
                (' ' | '\t', None) => break,
                ('"' | '\'', None) => quote = Some(c),
                (c, Some(q)) if c == q => quote = None,
                (c, _) => arg.push(c),
            }
            i += 1;
        }
        out.push(arg);
    }
    out
}

/// The value of a single-argument option as OpenSSH reads it (quotes and escapes removed).
/// Values that are not exactly one argument are returned trimmed, as written.
pub fn unquote(value: &str) -> String {
    match split_args(value).as_slice() {
        [one] => one.clone(),
        _ => value.trim().to_string(),
    }
}

/// Formats `v` as one ssh_config argument that [`split_args`] (and OpenSSH) read back as `v`.
/// Plain values, including Windows paths, are left unquoted.
pub fn quote_arg(v: &str) -> String {
    let quoted = v.is_empty()
        || v.starts_with('#')
        || v.chars()
            .any(|c| c.is_whitespace() || c == '"' || c == '\'');
    let chars: Vec<char> = v.chars().collect();
    let mut out = String::with_capacity(v.len() + 2);
    if quoted {
        out.push('"');
    }
    for (i, &c) in chars.iter().enumerate() {
        let next = chars.get(i + 1).copied();
        match c {
            // A backslash is literal unless it would form an escape (or end a quoted value).
            '\\' if matches!(next, Some('\\' | '"' | '\'')) || (quoted && next.is_none()) => {
                out.push_str("\\\\")
            }
            '"' => out.push_str("\\\""),
            c => out.push(c),
        }
    }
    if quoted {
        out.push('"');
    }
    out
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
        assert_eq!(unquote("'single quoted'"), "single quoted");
        assert_eq!(unquote("value # comment"), "value");
        assert_eq!(unquote("two args"), "two args");
    }

    #[test]
    fn follows_openssh_escapes() {
        // Unrecognised escapes (Windows paths) stay literal, inside and outside quotes.
        assert_eq!(split_args(r"C:\Users\me\id"), [r"C:\Users\me\id"]);
        assert_eq!(split_args(r#""C:\Users\me""#), [r"C:\Users\me"]);
        assert_eq!(
            split_args(r#"a\\b \"x\" it\'s"#),
            [r"a\b", r#""x""#, "it's"]
        );
        assert_eq!(split_args(r"a\ b"), ["a b"]);
        assert_eq!(split_args(r#""it's""#), ["it's"]);
        assert_eq!(split_args("a 'b \"c' d"), ["a", "b \"c", "d"]);
        assert_eq!(split_args("x #y"), ["x"]);
        assert_eq!(split_args("x#y"), ["x#y"]);
    }

    #[test]
    fn quote_arg_round_trips() {
        let values = [
            "plain",
            r"C:\Users\me\.ssh\id_ed25519",
            r"C:\Program Files\keys\id rsa",
            r"\\server\share\key",
            r"C:\dir\",
            r"ends with space\ ",
            r#"has "quotes" inside"#,
            "it's",
            r"back\\slashes\\",
            r#"\"tricky\'"#,
            "#hash",
            "a#b",
            "",
            "tab\there",
        ];
        for v in values {
            let q = quote_arg(v);
            assert_eq!(split_args(&q), [v], "{v:?} was written as {q}");
            assert_eq!(unquote(&q), v);
        }
        assert_eq!(quote_arg(r"C:\Users\me\id"), r"C:\Users\me\id");
        assert_eq!(quote_arg("id rsa"), "\"id rsa\"");
    }

    #[test]
    fn with_value_keeps_layout() {
        assert_eq!(
            Line::parse("  HostName  old").with_value("new"),
            "  HostName  new"
        );
        assert_eq!(
            Line::parse("\tport=22   ").with_value("2222"),
            "\tport=2222   "
        );
        assert_eq!(Line::parse("User = a b").with_value("c"), "User = c");
        assert_eq!(Line::parse("Host web").with_value("api"), "Host api");
        assert_eq!(
            Line::parse("ForwardAgent").with_value("yes"),
            "ForwardAgent yes"
        );
    }
}
