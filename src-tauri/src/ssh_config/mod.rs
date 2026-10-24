//! Lossless ssh_config parser (M2). The writer that applies edits lands in M4.
//!
//! A file is kept as its original lines plus a block structure on top. Untouched lines are
//! never regenerated, so comments, ordering, spacing, `Match` blocks and line endings survive.

pub mod lexer;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::db::hosts::Directive;
use crate::error::{AppError, AppResult};
use crate::security::validate_alias;
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
    #[allow(dead_code)] // used by the config writer (M4)
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
    #[cfg_attr(not(test), allow(dead_code))] // used by the config writer (M4)
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

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedConfig {
    #[serde(skip)]
    pub files: Vec<ConfigFile>,
    pub hosts: Vec<ConfigHost>,
    pub rules: Vec<RuleBlock>,
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

fn read_text(path: &Path) -> AppResult<String> {
    let bytes = std::fs::read(path)?;
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
        BlockKind::Match { criteria } => out.rules.push(rule("match", criteria.clone())),
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

fn extract_host(
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
        fingerprint: String::new(),
    };
    let mut hasher = Sha256::new();
    for line in file.directives(block) {
        let LineKind::Directive { key, value } = &line.kind else {
            continue;
        };
        hasher.update(key.to_ascii_lowercase().as_bytes());
        hasher.update([0]);
        hasher.update(value.as_bytes());
        hasher.update(b"\n");

        // OpenSSH uses the first value for single-valued options; later ones are kept verbatim.
        let slot = match key.to_ascii_lowercase().as_str() {
            "hostname" => Some(&mut h.hostname),
            "user" => Some(&mut h.user),
            "identityfile" => Some(&mut h.identity_file),
            "proxyjump" => Some(&mut h.proxy_jump),
            "port" if h.port.is_none() => match unquote(value).parse::<u16>() {
                Ok(p) if p > 0 => {
                    h.port = Some(p);
                    continue;
                }
                _ => {
                    warnings.push(format!(
                        "Host {alias}: invalid Port {value:?} kept as an option"
                    ));
                    None
                }
            },
            _ => None,
        };
        match slot {
            Some(s) if s.is_none() => *s = Some(unquote(value)),
            _ => h.extra_options.push(Directive {
                key: key.clone(),
                value: value.clone(),
            }),
        }
    }
    h.fingerprint = hex::encode(&hasher.finalize()[..12]);
    h
}

#[cfg(test)]
mod tests;

/// Lets other modules' tests build a [`ParsedConfig`] from text without touching the disk.
#[cfg(test)]
pub fn collect_block_for_tests(file: &ConfigFile, block: &Block, out: &mut ParsedConfig) {
    collect_block(file, block, out);
}
