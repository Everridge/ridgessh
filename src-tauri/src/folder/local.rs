//! Zipping a folder on this PC (uploads), with the same layout as the server side's
//! `zip -r -y`: entries are `<name>/…`, Unix permissions and symlinks are kept (on Unix),
//! and modification times are kept. Pure Rust, so Windows needs no `zip` tool.

use std::fs::{self, File};
use std::io::{self, BufWriter};
use std::path::Path;

use chrono::{DateTime as ChronoTime, Datelike, Local, Timelike};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipWriter};

use crate::error::{AppError, AppResult};

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PackReport {
    pub files: u64,
    /// Bytes of file content (before compression).
    pub bytes: u64,
    /// Entries that could not be read and were left out.
    pub skipped: Vec<String>,
    /// Entries left out by an exclude pattern (a folder counts once).
    pub excluded: u64,
}

/// Total size and file count of a folder (symlinks are not followed).
pub fn folder_size(dir: &Path) -> (u64, u64) {
    let mut files = 0;
    let mut bytes = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            let Ok(meta) = fs::symlink_metadata(e.path()) else {
                continue;
            };
            if meta.is_dir() {
                stack.push(e.path());
            } else if meta.is_file() {
                files += 1;
                bytes += meta.len();
            }
        }
    }
    (files, bytes)
}

fn zip_time(meta: &fs::Metadata) -> Option<DateTime> {
    let t: ChronoTime<Local> = meta.modified().ok()?.into();
    DateTime::from_date_and_time(
        u16::try_from(t.year()).ok()?,
        t.month() as u8,
        t.day() as u8,
        t.hour() as u8,
        t.minute() as u8,
        t.second() as u8,
    )
    .ok()
}

fn options(meta: &fs::Metadata) -> SimpleFileOptions {
    let mut o = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .large_file(true);
    if let Some(t) = zip_time(meta) {
        o = o.last_modified_time(t);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        o = o.unix_permissions(meta.permissions().mode() & 0o7777);
    }
    o
}

/// Zips `dir` into `out` as `<dir name>/…`, leaving out entries whose archive path matches
/// one of `excludes` (see `exclude_patterns`; an excluded folder is not entered).
pub fn pack(dir: &Path, out: &Path, excludes: &[String]) -> AppResult<PackReport> {
    let excludes: Vec<glob::Pattern> = excludes
        .iter()
        .map(|p| glob::Pattern::new(p))
        .collect::<Result<_, _>>()
        .map_err(|e| AppError::Validation(format!("exclude pattern: {e}")))?;
    let excluded = |entry: &str| excludes.iter().any(|p| p.matches(entry));
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| AppError::Validation(format!("{} is not a folder", dir.display())))?;
    let file = File::create(out)?;
    let mut zip = ZipWriter::new(BufWriter::new(file));
    let mut report = PackReport::default();
    let zip_err = |e: zip::result::ZipError| AppError::Other(format!("zip: {e}"));

    zip.add_directory(format!("{name}/"), options(&fs::metadata(dir)?))
        .map_err(zip_err)?;
    // Depth-first, sorted, so archives of the same folder are identical.
    let mut stack = vec![(dir.to_path_buf(), name.clone())];
    while let Some((path, entry)) = stack.pop() {
        let mut children: Vec<_> = match fs::read_dir(&path) {
            Ok(r) => r.flatten().collect(),
            Err(e) => {
                report.skipped.push(format!("{entry}/: {e}"));
                continue;
            }
        };
        children.sort_by_key(|c| c.file_name());
        let mut dirs = Vec::new();
        for c in children {
            let child = format!("{entry}/{}", c.file_name().to_string_lossy());
            if excluded(&child) {
                report.excluded += 1;
                continue;
            }
            let meta = match fs::symlink_metadata(c.path()) {
                Ok(m) => m,
                Err(e) => {
                    report.skipped.push(format!("{child}: {e}"));
                    continue;
                }
            };
            if meta.file_type().is_symlink() {
                #[cfg(unix)]
                {
                    let target = fs::read_link(c.path())?;
                    zip.add_symlink(child, target.to_string_lossy(), options(&meta))
                        .map_err(zip_err)?;
                }
                // Windows links need privileges to recreate and rarely matter here.
                #[cfg(not(unix))]
                report.skipped.push(format!("{child}: symbolic link"));
            } else if meta.is_dir() {
                zip.add_directory(format!("{child}/"), options(&meta))
                    .map_err(zip_err)?;
                dirs.push((c.path(), child));
            } else if meta.is_file() {
                let mut f = match File::open(c.path()) {
                    Ok(f) => f,
                    Err(e) => {
                        report.skipped.push(format!("{child}: {e}"));
                        continue;
                    }
                };
                zip.start_file(child, options(&meta)).map_err(zip_err)?;
                report.bytes += io::copy(&mut f, &mut zip)?;
                report.files += 1;
            }
        }
        // Reverse so the stack pops them in name order.
        stack.extend(dirs.into_iter().rev());
    }
    zip.finish().map_err(zip_err)?;
    Ok(report)
}
