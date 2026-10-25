//! Lossless ssh_config parser (M2) and the edit engine used by the guarded writer (M4).
//!
//! A file is kept as its original lines plus a block structure on top. Untouched lines are
//! never regenerated, so comments, ordering, spacing, `Match` blocks and line endings survive.

pub mod lexer;
pub mod writer;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::db::hosts::{Connection, Directive};
use crate::error::{AppError, AppResult};
use crate::security::{validate_alias, validate_user};
use lexer::{Line, LineKind, split_args, unquote};

/// OpenSSH refuses to nest includes deeper than this.
const MAX_INCLUDE_DEPTH: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum LineEnding {
    Lf,
    CrLf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockKind {
    /// Lines before the first `Host`/`Match` (global options, includes, comments).
    Preamble,
    Host {
        patterns: Vec<String>,
    },
    Match {
        criteria: String,
    },
}

/// A run of lines: the header line (except for the preamble) plus its body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub kind: BlockKind,
    /// Index of the first line (the header for Host/Match).
    pub start: usize,
    /// Exclusive end index.
    pub end: usize,
}

/// One file, exactly as read.
#[derive(Debug, Clone)]
pub struct ConfigFile {
    pub path: PathBuf,
    pub lines: Vec<Line>,
    /// Per line: was it terminated by CRLF (vs LF)? Keeps mixed-ending files lossless.
    pub crlf: Vec<bool>,
    /// Dominant ending, used for lines RidgeSSH adds.
    pub line_ending: LineEnding,
    pub bom: bool,
    pub trailing_newline: bool,
    pub blocks: Vec<Block>,
}

impl ConfigFile {
    pub fn parse(path: impl Into<PathBuf>, text: &str) -> Self {
        let (bom, text) = match text.strip_prefix('\u{feff}') {
            Some(t) => (true, t),
            None => (false, text),
        };
        let trailing_newline = text.ends_with('\n');
        let body = text.strip_suffix('\n').unwrap_or(text);
        let (mut lines, mut crlf) = (Vec::new(), Vec::new());
        if !text.is_empty() {
            let segments: Vec<&str> = body.split('\n').collect();
            let last = segments.len() - 1;
            for (i, seg) in segments.into_iter().enumerate() {
                // A trailing \r on a final unterminated line is content, not a line ending.
                let terminated = i < last || trailing_newline;
                let (raw, cr) = match seg.strip_suffix('\r') {
                    Some(s) if terminated => (s, true),
                    _ => (seg, false),
                };
                lines.push(Line::parse(raw));
                crlf.push(cr);
            }
        }
        let crlf_count = crlf.iter().filter(|c| **c).count();
        let line_ending = if crlf_count * 2 > crlf.len() {
            LineEnding::CrLf
        } else {
            LineEnding::Lf
        };
        let blocks = split_blocks(&lines);
        Self {
            path: path.into(),
            lines,
            crlf,
            line_ending,
            bom,
            trailing_newline,
            blocks,
        }
    }

    /// Reproduces the file. For an unmodified file this equals the original text.
    pub fn render(&self) -> String {
        let mut out = String::new();
        if self.bom {
            out.push('\u{feff}');
        }
        let last = self.lines.len().saturating_sub(1);
        for (i, line) in self.lines.iter().enumerate() {
            out.push_str(&line.raw);
            if i < last || self.trailing_newline {
                out.push_str(if self.crlf[i] { "\r\n" } else { "\n" });
            }
        }
        out
    }

    pub fn directives(&self, block: &Block) -> impl Iterator<Item = &Line> {
        let first = if block.kind == BlockKind::Preamble {
            block.start
        } else {
            block.start + 1
        };
        self.lines[first..block.end]
            .iter()
            .filter(|l| matches!(l.kind, LineKind::Directive { .. }))
    }
}

fn split_blocks(lines: &[Line]) -> Vec<Block> {
    let mut blocks = vec![Block {
        kind: BlockKind::Preamble,
        start: 0,
        end: lines.len(),
    }];
    for (i, line) in lines.iter().enumerate() {
        let kind = match line.keyword().as_deref() {
            Some("host") => BlockKind::Host {
                patterns: split_args(line.value().unwrap_or("")),
            },
            Some("match") => BlockKind::Match {
                criteria: line.value().unwrap_or("").to_string(),
            },
            _ => continue,
        };
        if let Some(prev) = blocks.last_mut() {
            prev.end = i;
        }
        blocks.push(Block {
            kind,
            start: i,
            end: lines.len(),
        });
    }
    blocks
}

/// A `Host` block with one concrete alias: the unit RidgeSSH manages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigHost {
    pub alias: String,
    pub hostname: Option<String>,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub identity_file: Option<String>,
    pub proxy_jump: Option<String>,
    pub extra_options: Vec<Directive>,
    pub source: PathBuf,
    /// 1-based line of the `Host` header.
    pub line: usize,
    pub fingerprint: String,
}

/// A block shown read-only: wildcard/multi-pattern `Host`, `Match`, or a duplicate alias.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleBlock {
    pub kind: &'static str,
    pub header: String,
    pub source: PathBuf,
    pub line: usize,
    pub directive_count: usize,
}

/// A `Match originalhost <alias> user <name>` block (criteria in either order): the settings
/// for one user of a managed host, typically its own `IdentityFile`. It applies when
/// connecting as `ssh name@alias`, so that is how RidgeSSH connects to it. Read-only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserVariant {
    pub alias: String,
    pub user: String,
    /// The block's first `IdentityFile`, if any.
    pub identity_file: Option<String>,
    /// Every directive of the block, for display.
    pub options: Vec<Directive>,
    pub source: PathBuf,
    /// 1-based line of the `Match` header.
    pub line: usize,
}

/// `originalhost X user Y` or `user Y originalhost X`, with a concrete alias and user name.
/// `host` is not accepted: it matches the name after `HostName`, not what was typed.
pub fn user_variant_criteria(criteria: &str) -> Option<(String, String)> {
    let args = split_args(criteria);
    if args.len() != 4 {
        return None;
    }
    let (mut host, mut user) = (None, None);
    for pair in args.chunks(2) {
        match pair[0].to_ascii_lowercase().as_str() {
            "originalhost" if host.is_none() => host = Some(pair[1].clone()),
            "user" if user.is_none() => user = Some(pair[1].clone()),
            _ => return None,
        }
    }
    let (host, user) = (host?, user?);
    (is_concrete_alias(&host) && validate_user(&user).is_ok()).then_some((host, user))
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedConfig {
    #[serde(skip)]
    pub files: Vec<ConfigFile>,
    pub hosts: Vec<ConfigHost>,
    pub rules: Vec<RuleBlock>,
    /// Per-user `Match` blocks of managed hosts (also listed in `rules`).
    pub variants: Vec<UserVariant>,
    pub warnings: Vec<String>,
}

/// Reads `path` and every file it includes. A missing main file is an empty config.
pub fn load(path: &Path) -> AppResult<ParsedConfig> {
    let mut out = ParsedConfig::default();
    if !path.exists() {
        out.warnings
            .push(format!("{} does not exist yet", path.display()));
        return Ok(out);
    }
    let base = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut seen = HashSet::new();
    load_file(path, &base, 0, &mut seen, &mut out)?;
    Ok(out)
}

/// Parses one config file on its own, without following `Include` (used for files from
/// another machine, whose include paths mean nothing here).
pub fn parse_single(path: &Path, text: &str) -> ParsedConfig {
    let mut out = ParsedConfig::default();
    let file = ConfigFile::parse(path, text);
    for block in &file.blocks {
        for line in file.directives(block) {
            if line.keyword().as_deref() == Some("include") {
                out.warnings.push(format!(
                    "Include {} was not followed; import that file separately",
                    line.value().unwrap_or("")
                ));
            }
        }
        collect_block(&file, block, &mut out);
    }
    out.files.push(file);
    out
}

pub fn read_text(path: &Path) -> AppResult<String> {
    decode(path, std::fs::read(path)?)
}

/// Config files must be UTF-8 (with or without BOM), like OpenSSH expects.
pub fn decode(path: &Path, bytes: Vec<u8>) -> AppResult<String> {
    if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        return Err(AppError::Validation(format!(
            "{} is UTF-16 encoded; OpenSSH and RidgeSSH need UTF-8",
            path.display()
        )));
    }
    String::from_utf8(bytes)
        .map_err(|_| AppError::Validation(format!("{} is not valid UTF-8", path.display())))
}

fn load_file(
    path: &Path,
    base: &Path,
    depth: usize,
    seen: &mut HashSet<PathBuf>,
    out: &mut ParsedConfig,
) -> AppResult<()> {
    let key = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if !seen.insert(key) {
        out.warnings.push(format!(
            "{} is included more than once; skipped",
            path.display()
        ));
        return Ok(());
    }
    let file = ConfigFile::parse(path, &read_text(path)?);

    let mut includes = Vec::new();
    for block in &file.blocks {
        for line in file.directives(block) {
            if line.keyword().as_deref() == Some("include") {
                includes.extend(split_args(line.value().unwrap_or("")));
            }
        }
        collect_block(&file, block, out);
    }
    out.files.push(file);

    for pattern in includes {
        if depth + 1 >= MAX_INCLUDE_DEPTH {
            out.warnings
                .push(format!("Include nesting too deep at {pattern}; skipped"));
            continue;
        }
        let resolved = resolve_include(&pattern, base);
        let mut matched = false;
        if let Ok(paths) = glob::glob(&resolved.to_string_lossy()) {
            for p in paths.flatten().filter(|p| p.is_file()) {
                matched = true;
                if let Err(e) = load_file(&p, base, depth + 1, seen, out) {
                    out.warnings
                        .push(format!("Could not read {}: {e}", p.display()));
                }
            }
        }
        if !matched && !pattern.contains(['*', '?']) {
            out.warnings
                .push(format!("Included file not found: {pattern}"));
        }
    }
    Ok(())
}

/// `~/x` → home, relative → relative to the main config's directory (as OpenSSH does).
fn resolve_include(pattern: &str, base: &Path) -> PathBuf {
    if let Some(rest) = pattern
        .strip_prefix("~/")
        .or_else(|| pattern.strip_prefix("~\\"))
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest);
    }
    let p = PathBuf::from(pattern);
    if p.is_absolute() { p } else { base.join(p) }
}

fn is_concrete_alias(pattern: &str) -> bool {
    validate_alias(pattern).is_ok()
}

fn collect_block(file: &ConfigFile, block: &Block, out: &mut ParsedConfig) {
    let rule = |kind, header: String| RuleBlock {
        kind,
        header,
        source: file.path.clone(),
        line: block.start + 1,
        directive_count: file.directives(block).count(),
    };
    match &block.kind {
        BlockKind::Preamble => {}
        BlockKind::Match { criteria } => {
            out.rules.push(rule("match", criteria.clone()));
            if let Some((alias, user)) = user_variant_criteria(criteria) {
                let options: Vec<Directive> = file
                    .directives(block)
                    .filter_map(|l| match &l.kind {
                        LineKind::Directive { key, value } => Some(Directive {
                            key: key.clone(),
                            value: value.clone(),
                        }),
                        _ => None,
                    })
                    .collect();
                let identity_file = options
                    .iter()
                    .find(|d| d.key.eq_ignore_ascii_case("identityfile"))
                    .map(|d| unquote(&d.value));
                out.variants.push(UserVariant {
                    alias,
                    user,
                    identity_file,
                    options,
                    source: file.path.clone(),
                    line: block.start + 1,
                });
            }
        }
        BlockKind::Host { patterns } => {
            let header = file.lines[block.start].value().unwrap_or("").to_string();
            if patterns.len() != 1 || !is_concrete_alias(&patterns[0]) {
                out.rules.push(rule("host", header));
                return;
            }
            let alias = &patterns[0];
            if let Some(first) = out.hosts.iter().find(|h| &h.alias == alias) {
                out.warnings.push(format!(
                    "Host {alias} is defined again at {}:{}; only the first block (line {}) is managed",
                    file.path.display(),
                    block.start + 1,
                    first.line
                ));
                out.rules.push(rule("host", header));
                return;
            }
            out.hosts
                .push(extract_host(file, block, alias, &mut out.warnings));
        }
    }
}

/// What a directive line of a managed `Host` block maps to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Slot {
    HostName,
    User,
    Port,
    IdentityFile,
    ProxyJump,
    /// Index into `extra_options`.
    Extra(usize),
}

/// Maps every directive line of a host block (by line index) to the field it fills.
/// OpenSSH uses the first value of single-valued options; later ones are kept verbatim as
/// extra options, and so is an unparsable `Port`. The writer relies on this exact mapping.
pub fn block_slots(file: &ConfigFile, block: &Block) -> Vec<(usize, Slot)> {
    let first = if block.kind == BlockKind::Preamble {
        block.start
    } else {
        block.start + 1
    };
    let mut taken = HashSet::new();
    let mut extras = 0;
    let mut out = Vec::new();
    for i in first..block.end {
        let LineKind::Directive { key, value } = &file.lines[i].kind else {
            continue;
        };
        let slot = match key.to_ascii_lowercase().as_str() {
            "hostname" => Some(Slot::HostName),
            "user" => Some(Slot::User),
            "identityfile" => Some(Slot::IdentityFile),
            "proxyjump" => Some(Slot::ProxyJump),
            "port" if matches!(unquote(value).parse::<u16>(), Ok(p) if p > 0) => Some(Slot::Port),
            _ => None,
        }
        .filter(|s| taken.insert(*s))
        .unwrap_or_else(|| {
            extras += 1;
            Slot::Extra(extras - 1)
        });
        out.push((i, slot));
    }
    out
}

/// Hash of a block's directives (keyword case and layout do not matter).
fn fingerprint(file: &ConfigFile, block: &Block) -> String {
    let mut hasher = Sha256::new();
    for line in file.directives(block) {
        if let LineKind::Directive { key, value } = &line.kind {
            hasher.update(key.to_ascii_lowercase().as_bytes());
            hasher.update([0]);
            hasher.update(value.as_bytes());
            hasher.update(b"\n");
        }
    }
    hex::encode(&hasher.finalize()[..12])
}

pub fn extract_host(
    file: &ConfigFile,
    block: &Block,
    alias: &str,
    warnings: &mut Vec<String>,
) -> ConfigHost {
    let mut h = ConfigHost {
        alias: alias.to_string(),
        hostname: None,
        user: None,
        port: None,
        identity_file: None,
        proxy_jump: None,
        extra_options: Vec::new(),
        source: file.path.clone(),
        line: block.start + 1,
        fingerprint: fingerprint(file, block),
    };
    for (i, slot) in block_slots(file, block) {
        let LineKind::Directive { key, value } = &file.lines[i].kind else {
            continue;
        };
        let v = || Some(unquote(value));
        match slot {
            Slot::HostName => h.hostname = v(),
            Slot::User => h.user = v(),
            Slot::IdentityFile => h.identity_file = v(),
            Slot::ProxyJump => h.proxy_jump = v(),
            Slot::Port => h.port = unquote(value).parse().ok(),
            Slot::Extra(_) => {
                if key.eq_ignore_ascii_case("port") && h.port.is_none() {
                    warnings.push(format!(
                        "Host {alias}: invalid Port {value:?} kept as an option"
                    ));
                }
                h.extra_options.push(Directive {
                    key: key.clone(),
                    value: value.clone(),
                });
            }
        }
    }
    h
}

impl ConfigHost {
    pub fn connection(&self) -> Connection {
        Connection {
            alias: self.alias.clone(),
            hostname: self.hostname.clone(),
            user: self.user.clone(),
            port: self.port,
            identity_file: self.identity_file.clone(),
            proxy_jump: self.proxy_jump.clone(),
            extra_options: self.extra_options.clone(),
        }
    }
}

/// The managed block for `alias` in one file: the first `Host` block whose only pattern is it.
pub fn find_host_block<'a>(file: &'a ConfigFile, alias: &str) -> Option<&'a Block> {
    file.blocks.iter().find(
        |b| matches!(&b.kind, BlockKind::Host { patterns } if patterns.len() == 1 && patterns[0] == alias),
    )
}

#[cfg(test)]
mod tests;

/// Lets other modules' tests build a [`ParsedConfig`] from text without touching the disk.
#[cfg(test)]
pub fn collect_block_for_tests(file: &ConfigFile, block: &Block, out: &mut ParsedConfig) {
    collect_block(file, block, out);
}
