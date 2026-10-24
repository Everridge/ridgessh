//! Applies host edits to a parsed config file and returns the new text (M4).
//!
//! Only the lines that carry a changed value are touched: comments, ordering, indentation,
//! keyword spelling, `key=value` style, line endings and every other block are copied
//! verbatim. The result is re-parsed and checked by [`verify`] before it can be written.

use std::collections::{BTreeMap, HashMap, HashSet};

use similar::{Algorithm, DiffOp, capture_diff_slices};

use super::lexer::{LineKind, quote_arg};
use super::{
    Block, BlockKind, ConfigFile, LineEnding, ParsedConfig, Slot, block_slots, collect_block,
    extract_host, find_host_block,
};
use crate::db::hosts::Connection;
use crate::error::{AppError, AppResult};
use crate::security::validate_alias;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edit {
    /// Rewrites the block that the config currently calls `alias` so it matches `conn`
    /// (which may carry a new alias).
    Update { alias: String, conn: Connection },
    /// Adds a new `Host` block after the last managed host (before trailing `Host *` rules).
    Add { conn: Connection },
    /// Deletes the block called `alias`.
    Remove { alias: String },
}

const DEFAULT_INDENT: &str = "    ";

/// Applies `edits` to `file`. `other_aliases` are hosts defined in included files, which a
/// new or renamed host must not shadow.
pub fn apply(
    file: &ConfigFile,
    edits: &[Edit],
    other_aliases: &HashSet<String>,
) -> AppResult<String> {
    let mut by_block: HashMap<usize, &Edit> = HashMap::new();
    let mut adds = Vec::new();
    for edit in edits {
        let target = match edit {
            Edit::Add { conn } => {
                check_connection(conn)?;
                check_alias_free(file, &conn.alias, None, other_aliases)?;
                adds.push(conn);
                continue;
            }
            Edit::Update { alias, conn } => {
                check_connection(conn)?;
                let idx = block_index(file, alias)?;
                if conn.alias != *alias {
                    check_alias_free(file, &conn.alias, Some(idx), other_aliases)?;
                }
                idx
            }
            Edit::Remove { alias } => block_index(file, alias)?,
        };
        if by_block.insert(target, edit).is_some() {
            return Err(AppError::Validation(
                "the same host is changed twice".into(),
            ));
        }
    }
    let mut new_aliases = HashSet::new();
    for conn in adds
        .iter()
        .copied()
        .chain(edits.iter().filter_map(|e| match e {
            Edit::Update { conn, .. } => Some(conn),
            _ => None,
        }))
    {
        if !new_aliases.insert(conn.alias.as_str()) {
            return Err(AppError::Conflict(format!(
                "two hosts would be named \"{}\"",
                conn.alias
            )));
        }
    }

    let nl = file.line_ending == LineEnding::CrLf;
    let indent = default_indent(file);
    let anchor = add_anchor(file);
    let mut out = Out::new(file);

    for (bi, block) in file.blocks.iter().enumerate() {
        let head_end = content_end(file, block);
        let removed = matches!(by_block.get(&bi), Some(Edit::Remove { .. }));
        match by_block.get(&bi) {
            Some(Edit::Update { alias, conn }) => {
                rewrite_block(file, block, head_end, alias, conn, &indent, &mut out)
            }
            Some(Edit::Remove { .. }) => {}
            _ => (block.start..head_end).for_each(|i| out.copy(file, i)),
        }
        if anchor == Some(bi) {
            out.add_blocks(&adds, &indent, nl);
        }
        let mut tail = head_end..block.end;
        if removed {
            // Don't leave two blank lines where the block used to be.
            while tail.start < tail.end
                && file.lines[tail.start].kind == LineKind::Blank
                && out.last_is_blank_or_empty()
            {
                tail.start += 1;
            }
        }
        tail.for_each(|i| out.copy(file, i));
    }
    if anchor.is_none() {
        out.add_blocks(&adds, &indent, nl);
    }
    // A removed last block can leave blank lines at the end of a file that had none.
    let ended_blank = file.lines.last().is_some_and(|l| l.kind == LineKind::Blank);
    while !ended_blank && out.lines.last().is_some_and(|l| l.trim().is_empty()) {
        out.lines.pop();
        out.crlf.pop();
    }
    Ok(out.render(file))
}

fn block_index(file: &ConfigFile, alias: &str) -> AppResult<usize> {
    let block = find_host_block(file, alias).ok_or_else(|| {
        AppError::Conflict(format!(
            "Host {alias} is no longer in {}. Re-scan the config first.",
            file.path.display()
        ))
    })?;
    Ok(file
        .blocks
        .iter()
        .position(|b| b == block)
        .unwrap_or_default())
}

/// A new alias must not already be matched literally by another `Host` line.
fn check_alias_free(
    file: &ConfigFile,
    alias: &str,
    own_block: Option<usize>,
    other_aliases: &HashSet<String>,
) -> AppResult<()> {
    if other_aliases.contains(alias) {
        return Err(AppError::Conflict(format!(
            "Host {alias} is already defined in an included config file"
        )));
    }
    for (i, b) in file.blocks.iter().enumerate() {
        if Some(i) == own_block {
            continue;
        }
        if let BlockKind::Host { patterns } = &b.kind
            && patterns.iter().any(|p| p == alias)
        {
            return Err(AppError::Conflict(format!(
                "the config already has a Host line for {alias} (line {})",
                b.start + 1
            )));
        }
    }
    Ok(())
}

/// Everything written must stay a single, well-formed directive.
fn check_connection(c: &Connection) -> AppResult<()> {
    validate_alias(&c.alias)?;
    let fields = [
        ("HostName", &c.hostname),
        ("User", &c.user),
        ("IdentityFile", &c.identity_file),
        ("ProxyJump", &c.proxy_jump),
    ];
    for (key, v) in fields {
        if let Some(v) = v
            && (v.is_empty() || v.chars().any(char::is_control))
        {
            return Err(AppError::Validation(format!(
                "invalid {key} for {}",
                c.alias
            )));
        }
    }
    if c.port == Some(0) {
        return Err(AppError::Validation(format!(
            "invalid Port for {}",
            c.alias
        )));
    }
    for d in &c.extra_options {
        let bad_key = d.key.is_empty()
            || !d.key.chars().all(|ch| ch.is_ascii_alphanumeric())
            // These would start a new block and change the meaning of the rest of the file.
            || d.key.eq_ignore_ascii_case("host")
            || d.key.eq_ignore_ascii_case("match");
        if bad_key || d.value.chars().any(char::is_control) {
            return Err(AppError::Validation(format!(
                "invalid option {:?} for {}",
                d.key, c.alias
            )));
        }
    }
    Ok(())
}

/// Exclusive end of the block's own lines: up to its last directive. Trailing blank lines and
/// comments usually introduce the next block, so they stay with it.
fn content_end(file: &ConfigFile, block: &Block) -> usize {
    (block.start..block.end)
        .rev()
        .find(|&i| matches!(file.lines[i].kind, LineKind::Directive { .. }))
        .map_or(block.start, |i| i + 1)
}

/// New hosts go after the last managed host, so they stay ahead of `Host *` / `Match` rules
/// at the end of the file (OpenSSH uses the first value it finds for each option).
/// `None` means the end of the file.
fn add_anchor(file: &ConfigFile) -> Option<usize> {
    let last_host = file.blocks.iter().rposition(|b| {
        matches!(&b.kind, BlockKind::Host { patterns }
            if patterns.len() == 1 && super::is_concrete_alias(&patterns[0]))
    });
    match last_host {
        Some(i) => Some(i),
        // Only rules: right after the global options, before the first rule.
        None if file.blocks.len() > 1 => Some(0),
        None => None,
    }
}

fn default_indent(file: &ConfigFile) -> String {
    file.blocks
        .iter()
        .filter(|b| b.kind != BlockKind::Preamble)
        .flat_map(|b| file.lines[b.start + 1..b.end].iter())
        .find(|l| matches!(l.kind, LineKind::Directive { .. }) && !l.indent().is_empty())
        .map_or_else(|| DEFAULT_INDENT.to_string(), |l| l.indent().to_string())
}

/// Known single-value fields in the order new lines are written.
fn known_fields(c: &Connection) -> [(Slot, &'static str, Option<String>); 5] {
    [
        (Slot::HostName, "HostName", c.hostname.clone()),
        (Slot::User, "User", c.user.clone()),
        (Slot::Port, "Port", c.port.map(|p| p.to_string())),
        (Slot::IdentityFile, "IdentityFile", c.identity_file.clone()),
        (Slot::ProxyJump, "ProxyJump", c.proxy_jump.clone()),
    ]
}

fn rewrite_block(
    file: &ConfigFile,
    block: &Block,
    head_end: usize,
    alias: &str,
    conn: &Connection,
    default_indent: &str,
    out: &mut Out,
) {
    let current = extract_host(file, block, alias, &mut Vec::new()).connection();
    let slots = block_slots(file, block);
    let indent = slots
        .first()
        .map_or(default_indent, |(i, _)| file.lines[*i].indent())
        .to_string();
    let line_of = |slot: Slot| slots.iter().find(|(_, s)| *s == slot).map(|(i, _)| *i);

    // Per original line: replacement text (None = delete). Plus lines added after a line.
    let mut repl: BTreeMap<usize, Option<String>> = BTreeMap::new();
    let mut after: BTreeMap<usize, Vec<String>> = BTreeMap::new();

    if conn.alias != current.alias {
        repl.insert(
            block.start,
            Some(file.lines[block.start].with_value(&conn.alias)),
        );
    }

    let mut prev = block.start;
    for ((slot, key, want), (_, _, have)) in
        known_fields(conn).into_iter().zip(known_fields(&current))
    {
        let line = line_of(slot);
        match (line, &want) {
            (Some(i), Some(v)) if want != have => {
                repl.insert(i, Some(file.lines[i].with_value(&quote_arg(v))));
            }
            (Some(_), Some(_)) => {}
            (Some(i), None) => {
                repl.insert(i, None);
            }
            (None, Some(v)) => after
                .entry(prev)
                .or_default()
                .push(format!("{indent}{key} {}", quote_arg(v))),
            (None, None) => {}
        }
        if let Some(i) = line {
            prev = prev.max(i);
        }
    }

    // Extra options: align old and new lists so unchanged lines stay where they are.
    let extra_lines: Vec<usize> = slots
        .iter()
        .filter(|(_, s)| matches!(s, Slot::Extra(_)))
        .map(|(i, _)| *i)
        .collect();
    let norm = |d: &crate::db::hosts::Directive| (d.key.to_ascii_lowercase(), d.value.clone());
    let old: Vec<_> = current.extra_options.iter().map(norm).collect();
    let new: Vec<_> = conn.extra_options.iter().map(norm).collect();
    let new_line = |k: usize| {
        let d = &conn.extra_options[k];
        format!("{indent}{} {}", d.key, d.value)
    };
    // Where lines inserted before old extra `p` go.
    let insert_point = |p: usize| match p.checked_sub(1) {
        Some(q) => extra_lines[q],
        None => extra_lines.first().map_or(head_end - 1, |&l| l - 1),
    };
    for op in capture_diff_slices(Algorithm::Myers, &old, &new) {
        match op {
            DiffOp::Equal { .. } => {}
            DiffOp::Delete {
                old_index, old_len, ..
            } => {
                for &li in &extra_lines[old_index..old_index + old_len] {
                    repl.insert(li, None);
                }
            }
            DiffOp::Insert {
                old_index,
                new_index,
                new_len,
            } => {
                let at = insert_point(old_index);
                let lines = (new_index..new_index + new_len).map(new_line);
                after.entry(at).or_default().extend(lines);
            }
            DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => {
                let paired = old_len.min(new_len);
                for k in 0..paired {
                    let li = extra_lines[old_index + k];
                    let d = &conn.extra_options[new_index + k];
                    let line = &file.lines[li];
                    let same_key = line.keyword().as_deref() == Some(&d.key.to_ascii_lowercase());
                    let text = if same_key {
                        line.with_value(&d.value)
                    } else {
                        format!("{}{} {}", line.indent(), d.key, d.value)
                    };
                    repl.insert(li, Some(text));
                }
                for &li in &extra_lines[old_index + paired..old_index + old_len] {
                    repl.insert(li, None);
                }
                if new_len > paired {
                    // Replace ops are never empty on either side, so `paired >= 1`.
                    let at = extra_lines[old_index + paired - 1];
                    let lines = (new_index + paired..new_index + new_len).map(new_line);
                    after.entry(at).or_default().extend(lines);
                }
            }
        }
    }

    let nl = file.line_ending == LineEnding::CrLf;
    for i in block.start..head_end {
        match repl.get(&i) {
            Some(Some(text)) => out.push(text.clone(), out.crlf_of(file, i)),
            Some(None) => {}
            None => out.copy(file, i),
        }
        for text in after.remove(&i).unwrap_or_default() {
            out.push(text, nl);
        }
    }
}

/// Output lines with their line-ending flags.
struct Out {
    lines: Vec<String>,
    crlf: Vec<bool>,
    /// A blank line goes before the next copied line unless it is blank itself.
    separate_next: bool,
}

impl Out {
    fn new(file: &ConfigFile) -> Self {
        Self {
            lines: Vec::with_capacity(file.lines.len() + 8),
            crlf: Vec::with_capacity(file.lines.len() + 8),
            separate_next: false,
        }
    }

    /// Line ending for original line `i`. The unterminated last line of a file gets the
    /// file's usual ending once something follows it.
    fn crlf_of(&self, file: &ConfigFile, i: usize) -> bool {
        file.crlf[i] || (i + 1 == file.lines.len() && file.line_ending == LineEnding::CrLf)
    }

    fn push(&mut self, text: String, crlf: bool) {
        self.lines.push(text);
        self.crlf.push(crlf);
    }

    fn copy(&mut self, file: &ConfigFile, i: usize) {
        let line = &file.lines[i];
        if std::mem::take(&mut self.separate_next) && line.kind != LineKind::Blank {
            self.push(String::new(), file.line_ending == LineEnding::CrLf);
        }
        self.push(line.raw.clone(), self.crlf_of(file, i));
    }

    fn last_is_blank_or_empty(&self) -> bool {
        self.lines.last().is_none_or(|l| l.trim().is_empty())
    }

    fn add_blocks(&mut self, adds: &[&Connection], indent: &str, nl: bool) {
        for conn in adds {
            if !self.last_is_blank_or_empty() {
                self.push(String::new(), nl);
            }
            self.push(format!("Host {}", conn.alias), nl);
            for (_, key, value) in known_fields(conn) {
                if let Some(v) = value {
                    self.push(format!("{indent}{key} {}", quote_arg(&v)), nl);
                }
            }
            for d in &conn.extra_options {
                self.push(format!("{indent}{} {}", d.key, d.value), nl);
            }
            self.separate_next = true;
        }
    }

    fn render(&self, file: &ConfigFile) -> String {
        let mut s = String::new();
        if file.bom {
            s.push('\u{feff}');
        }
        // An empty file that gets hosts ends with a newline like any normal config.
        let trailing = file.trailing_newline || file.lines.is_empty();
        let last = self.lines.len().saturating_sub(1);
        for (i, line) in self.lines.iter().enumerate() {
            s.push_str(line);
            if i < last || trailing {
                s.push_str(if self.crlf[i] { "\r\n" } else { "\n" });
            }
        }
        s
    }
}

/// Re-parses `new_text` and checks that it says exactly what the edits intend: edited and
/// added hosts parse back to their target connection, removed ones are gone, and every
/// other host and rule is unchanged. Returns the parsed new file.
pub fn verify(old: &ConfigFile, new_text: &str, edits: &[Edit]) -> AppResult<ConfigFile> {
    let fail = |what: String| {
        Err(AppError::Other(format!(
            "internal check of the new config failed ({what}); nothing was written"
        )))
    };
    let new = ConfigFile::parse(old.path.clone(), new_text);
    if new.render() != new_text {
        return fail("it does not round-trip".into());
    }
    let collect = |f: &ConfigFile| {
        let mut p = ParsedConfig::default();
        for b in &f.blocks {
            collect_block(f, b, &mut p);
        }
        p
    };
    let (before, after) = (collect(old), collect(&new));

    let mut expected: BTreeMap<&str, Option<&Connection>> = before
        .hosts
        .iter()
        .map(|h| (h.alias.as_str(), None))
        .collect();
    for edit in edits {
        match edit {
            Edit::Update { alias, conn } => {
                expected.remove(alias.as_str());
                expected.insert(&conn.alias, Some(conn));
            }
            Edit::Add { conn } => {
                expected.insert(&conn.alias, Some(conn));
            }
            Edit::Remove { alias } => {
                expected.remove(alias.as_str());
            }
        }
    }
    let got: BTreeMap<&str, _> = after.hosts.iter().map(|h| (h.alias.as_str(), h)).collect();
    if got.keys().ne(expected.keys()) {
        return fail("the set of hosts differs".into());
    }
    for (alias, want) in &expected {
        let h = got[alias];
        let ok = match want {
            Some(conn) => h.connection() == **conn,
            None => before
                .hosts
                .iter()
                .any(|b| b.alias == *alias && b.fingerprint == h.fingerprint),
        };
        if !ok {
            return fail(format!("Host {alias} does not read back as expected"));
        }
    }
    let rule_key = |p: &ParsedConfig| {
        p.rules
            .iter()
            .map(|r| (r.kind, r.header.clone(), r.directive_count))
            .collect::<Vec<_>>()
    };
    if rule_key(&before) != rule_key(&after) {
        return fail("wildcard or Match blocks changed".into());
    }
    Ok(new)
}

#[cfg(test)]
mod tests;
