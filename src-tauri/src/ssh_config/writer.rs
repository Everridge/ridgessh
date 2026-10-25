//! Applies host edits to a parsed config file and returns the new text (M4).
//!
//! Only the lines that carry a changed value are touched: comments, ordering, indentation,
//! keyword spelling, `key=value` style, line endings and every other block are copied
//! verbatim. The result is re-parsed and checked by [`verify`] before it can be written.

use std::collections::{BTreeMap, HashMap, HashSet};

use similar::{Algorithm, DiffOp, capture_diff_slices};

use super::lexer::{LineKind, quote_arg, split_args, unquote};
use super::{
    Block, BlockKind, ConfigFile, LineEnding, ParsedConfig, Slot, attach_user_matches, block_slots,
    collect_block, extract_host, find_host_block, user_variant_criteria,
};
use crate::db::hosts::{Connection, Directive, UserMatch};
use crate::error::{AppError, AppResult};
use crate::security::{validate_alias, validate_user};

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

    let user_blocks = plan_user_blocks(file, &by_block);

    let nl = file.line_ending == LineEnding::CrLf;
    let indent = default_indent(file);
    let anchor = add_anchor(file);
    let mut out = Out::new(file);

    for (bi, block) in file.blocks.iter().enumerate() {
        let head_end = content_end(file, block);
        let removed = matches!(by_block.get(&bi), Some(Edit::Remove { .. }))
            || matches!(user_blocks.ops.get(&bi), Some(None));
        match (by_block.get(&bi), user_blocks.ops.get(&bi)) {
            (Some(Edit::Update { alias, conn }), _) => {
                rewrite_block(file, block, head_end, alias, conn, &indent, &mut out)
            }
            (Some(Edit::Remove { .. }), _) | (_, Some(None)) => {}
            (_, Some(Some((alias, m)))) => {
                rewrite_user_block(file, block, head_end, alias, m, &indent, &mut out)
            }
            _ => (block.start..head_end).for_each(|i| out.copy(file, i)),
        }
        if anchor == Some(bi) {
            out.add_blocks(&adds, &indent, nl);
        }
        if let Some(new) = user_blocks.adds.get(&bi) {
            for (alias, m) in new {
                out.add_user_block(alias, m, &indent, nl);
            }
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

/// What happens to per-user `Match` blocks (see [`super::UserVariant`]).
struct UserBlocks<'a> {
    /// Per existing block: rewrite it for (alias, settings), or delete it (`None`).
    ops: HashMap<usize, Option<(&'a str, &'a UserMatch)>>,
    /// New blocks, added after the block with this index.
    adds: HashMap<usize, Vec<(&'a str, &'a UserMatch)>>,
}

/// The per-user blocks in `file`: (block index, alias, user), in file order.
fn user_blocks(file: &ConfigFile) -> Vec<(usize, String, String)> {
    file.blocks
        .iter()
        .enumerate()
        .filter_map(|(i, b)| match &b.kind {
            BlockKind::Match { criteria } => {
                user_variant_criteria(criteria).map(|(alias, user)| (i, alias, user))
            }
            _ => None,
        })
        .collect()
}

/// Updated hosts keep, rewrite or lose their per-user blocks, and new users get a block after
/// the host's last one (or after the host's own block). Removed hosts lose them all.
fn plan_user_blocks<'a>(file: &ConfigFile, by_block: &HashMap<usize, &'a Edit>) -> UserBlocks<'a> {
    let existing = user_blocks(file);
    let mut plan = UserBlocks {
        ops: HashMap::new(),
        adds: HashMap::new(),
    };
    for (&host_block, edit) in by_block {
        match edit {
            Edit::Update { alias, conn } => {
                let mine: Vec<_> = existing.iter().filter(|(_, a, _)| a == alias).collect();
                for (i, _, user) in &mine {
                    let keep = conn.user_matches.iter().find(|m| &m.user == user);
                    plan.ops.insert(*i, keep.map(|m| (conn.alias.as_str(), m)));
                }
                let anchor = mine.last().map_or(host_block, |(i, _, _)| *i);
                let new: Vec<_> = conn
                    .user_matches
                    .iter()
                    .filter(|m| !mine.iter().any(|(_, _, u)| *u == m.user))
                    .map(|m| (conn.alias.as_str(), m))
                    .collect();
                if !new.is_empty() {
                    plan.adds.entry(anchor).or_default().extend(new);
                }
            }
            Edit::Remove { alias } => {
                for (i, _, _) in existing.iter().filter(|(_, a, _)| a == alias) {
                    plan.ops.insert(*i, None);
                }
            }
            Edit::Add { .. } => {}
        }
    }
    plan
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
    check_options(&c.extra_options, &c.alias)?;
    let mut users = HashSet::new();
    for m in &c.user_matches {
        let label = format!("{}@{}", m.user, c.alias);
        if validate_user(&m.user).is_err() || !users.insert(m.user.as_str()) {
            return Err(AppError::Validation(format!("invalid user {label}")));
        }
        if let Some(v) = &m.identity_file
            && (v.is_empty() || v.chars().any(char::is_control))
        {
            return Err(AppError::Validation(format!(
                "invalid IdentityFile for {label}"
            )));
        }
        check_options(&m.extra_options, &label)?;
    }
    Ok(())
}

fn check_options(options: &[Directive], owner: &str) -> AppResult<()> {
    for d in options {
        let bad_key = d.key.is_empty()
            || !d.key.chars().all(|ch| ch.is_ascii_alphanumeric())
            // These would start a new block and change the meaning of the rest of the file.
            || d.key.eq_ignore_ascii_case("host")
            || d.key.eq_ignore_ascii_case("match");
        if bad_key || d.value.chars().any(char::is_control) {
            return Err(AppError::Validation(format!(
                "invalid option {:?} for {owner}",
                d.key
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
        // After that host's own per-user blocks, which belong with it.
        Some(i) => {
            let BlockKind::Host { patterns } = &file.blocks[i].kind else {
                return Some(i);
            };
            let mine = user_blocks(file)
                .into_iter()
                .filter(|(_, alias, _)| *alias == patterns[0])
                .map(|(b, _, _)| b)
                .collect::<HashSet<_>>();
            let mut end = i;
            while mine.contains(&(end + 1)) {
                end += 1;
            }
            Some(end)
        }
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
    let header = (conn.alias != current.alias).then(|| conn.alias.clone());
    let fields = known_fields(conn)
        .into_iter()
        .zip(known_fields(&current))
        .map(|((slot, key, want), (_, _, have))| (slot, key, want, have))
        .collect();
    let body = Body {
        slots: block_slots(file, block),
        header,
        fields,
        old_extras: &current.extra_options,
        new_extras: &conn.extra_options,
    };
    rewrite_lines(file, block, head_end, body, default_indent, out);
}

/// The lines of a per-user block, mapped like [`block_slots`] but with `IdentityFile` as the
/// only field (the first one; the rest are options), as [`super::UserVariant::user_match`]
/// reads them.
fn user_block_slots(file: &ConfigFile, block: &Block) -> Vec<(usize, Slot)> {
    let mut identity = false;
    let mut extras = 0;
    let mut out = Vec::new();
    for i in block.start + 1..block.end {
        let LineKind::Directive { key, .. } = &file.lines[i].kind else {
            continue;
        };
        if !identity && key.eq_ignore_ascii_case("identityfile") {
            identity = true;
            out.push((i, Slot::IdentityFile));
        } else {
            out.push((i, Slot::Extra(extras)));
            extras += 1;
        }
    }
    out
}

/// Rewrites a per-user `Match` block for `alias` (renamed hosts get a new header) and `m`.
fn rewrite_user_block(
    file: &ConfigFile,
    block: &Block,
    head_end: usize,
    alias: &str,
    m: &UserMatch,
    default_indent: &str,
    out: &mut Out,
) {
    let slots = user_block_slots(file, block);
    let mut have_identity = None;
    let mut old_extras = Vec::new();
    for (i, slot) in &slots {
        if let LineKind::Directive { key, value } = &file.lines[*i].kind {
            match slot {
                Slot::IdentityFile => have_identity = Some(unquote(value)),
                _ => old_extras.push(Directive {
                    key: key.clone(),
                    value: value.clone(),
                }),
            }
        }
    }
    // The criteria with the alias swapped in, keeping their order.
    let criteria = file.lines[block.start].value().unwrap_or("");
    let header = match user_variant_criteria(criteria) {
        Some((old, _)) if old != alias => Some(
            split_args(criteria)
                .chunks(2)
                .flat_map(|pair| {
                    let value = if pair[0].eq_ignore_ascii_case("originalhost") {
                        alias.to_string()
                    } else {
                        pair[1].clone()
                    };
                    [pair[0].clone(), value]
                })
                .collect::<Vec<_>>()
                .join(" "),
        ),
        _ => None,
    };
    let body = Body {
        slots,
        header,
        fields: vec![(
            Slot::IdentityFile,
            "IdentityFile",
            m.identity_file.clone(),
            have_identity,
        )],
        old_extras: &old_extras,
        new_extras: &m.extra_options,
    };
    rewrite_lines(file, block, head_end, body, default_indent, out);
}

/// What a block should say: its lines mapped to fields, a new header value, the fields
/// (slot, keyword, wanted value, current value) in writing order, and the other options.
struct Body<'a> {
    slots: Vec<(usize, Slot)>,
    header: Option<String>,
    fields: Vec<(Slot, &'static str, Option<String>, Option<String>)>,
    old_extras: &'a [Directive],
    new_extras: &'a [Directive],
}

/// Writes `block` with only the lines that change replaced, removed or added.
fn rewrite_lines(
    file: &ConfigFile,
    block: &Block,
    head_end: usize,
    body: Body,
    default_indent: &str,
    out: &mut Out,
) {
    let Body {
        slots,
        header,
        fields,
        old_extras,
        new_extras,
    } = body;
    let indent = slots
        .first()
        .map_or(default_indent, |(i, _)| file.lines[*i].indent())
        .to_string();
    let line_of = |slot: Slot| slots.iter().find(|(_, s)| *s == slot).map(|(i, _)| *i);

    // Per original line: replacement text (None = delete). Plus lines added after a line.
    let mut repl: BTreeMap<usize, Option<String>> = BTreeMap::new();
    let mut after: BTreeMap<usize, Vec<String>> = BTreeMap::new();

    if let Some(h) = header {
        repl.insert(block.start, Some(file.lines[block.start].with_value(&h)));
    }

    let mut prev = block.start;
    for (slot, key, want, have) in fields {
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
    let norm = |d: &Directive| (d.key.to_ascii_lowercase(), d.value.clone());
    let old: Vec<_> = old_extras.iter().map(norm).collect();
    let new: Vec<_> = new_extras.iter().map(norm).collect();
    let new_line = |k: usize| {
        let d = &new_extras[k];
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
                    let d = &new_extras[new_index + k];
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
            for m in &conn.user_matches {
                self.add_user_block(&conn.alias, m, indent, nl);
            }
        }
    }

    /// `Match originalhost <alias> user <user>` with the user's settings.
    fn add_user_block(&mut self, alias: &str, m: &UserMatch, indent: &str, nl: bool) {
        if !self.last_is_blank_or_empty() {
            self.push(String::new(), nl);
        }
        self.push(format!("Match originalhost {alias} user {}", m.user), nl);
        if let Some(v) = &m.identity_file {
            self.push(format!("{indent}IdentityFile {}", quote_arg(v)), nl);
        }
        for d in &m.extra_options {
            self.push(format!("{indent}{} {}", d.key, d.value), nl);
        }
        self.separate_next = true;
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
        attach_user_matches(&mut p);
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
            None => before.hosts.iter().any(|b| {
                b.alias == *alias
                    && b.fingerprint == h.fingerprint
                    && b.user_matches == h.user_matches
            }),
        };
        if !ok {
            return fail(format!("Host {alias} does not read back as expected"));
        }
    }
    // Per-user blocks of hosts were checked with them above. No host that went away (removed
    // or renamed) may keep any; every other rule stays exactly as it was.
    let aliases =
        |p: &ParsedConfig| -> HashSet<String> { p.hosts.iter().map(|h| h.alias.clone()).collect() };
    let (before_aliases, after_aliases) = (aliases(&before), aliases(&after));
    if after
        .variants
        .iter()
        .any(|v| before_aliases.contains(&v.alias) && !after_aliases.contains(&v.alias))
    {
        return fail("a removed host kept its Match blocks".into());
    }
    let rule_key = |p: &ParsedConfig| {
        p.rules
            .iter()
            .filter(|r| {
                r.kind != "match"
                    || user_variant_criteria(&r.header).is_none_or(|(alias, _)| {
                        !before_aliases.contains(&alias) && !after_aliases.contains(&alias)
                    })
            })
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
