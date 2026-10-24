//! Script library and remote execution (M7).
//!
//! A script runs as one `ssh -t <host> <command>` in a terminal tab. The script travels
//! inside the command, base64-encoded, so it is never typed into the terminal and needs no
//! quoting; any remote shell just sees `sh -c '<fixed text>'`.
//! - **Stream**: `printf … | base64 -d | bash -s -- args`. Nothing is left on the server; the
//!   script's stdin is the pipe.
//! - **Copy & run**: decoded into a `mktemp` file, run as `bash "$f" args` with the terminal
//!   attached (so it can prompt, e.g. `sudo`), then deleted.

use base64::Engine;
use rusqlite::{OptionalExtension, Row, Transaction, params};
use serde::{Deserialize, Serialize};

use crate::db::{new_id, now};
use crate::error::{AppError, AppResult};
use crate::history::{self, Action, EntityType, Source};
use crate::ssh_config::lexer::split_args;
use crate::terminal::ssh_args::posix_quote;

/// Windows limits a command line to 32 767 characters; base64 adds a third.
pub const MAX_SCRIPT_BYTES: usize = 20_000;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum RunMode {
    #[default]
    Stream,
    Copy,
}

impl RunMode {
    fn as_str(self) -> &'static str {
        match self {
            RunMode::Stream => "stream",
            RunMode::Copy => "copy",
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Script {
    pub id: String,
    pub name: String,
    pub description: String,
    pub content: String,
    pub interpreter: String,
    pub default_args: String,
    pub run_mode: RunMode,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptInput {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub content: String,
    #[serde(default = "default_interpreter")]
    pub interpreter: String,
    #[serde(default)]
    pub default_args: String,
    #[serde(default)]
    pub run_mode: RunMode,
}

fn default_interpreter() -> String {
    "bash".into()
}

const COLUMNS: &str =
    "id, name, description, content, interpreter, default_args, run_mode, created_at, updated_at";

fn from_row(r: &Row) -> rusqlite::Result<Script> {
    let mode: String = r.get(6)?;
    Ok(Script {
        id: r.get(0)?,
        name: r.get(1)?,
        description: r.get(2)?,
        content: r.get(3)?,
        interpreter: r.get(4)?,
        default_args: r.get(5)?,
        run_mode: if mode == "copy" {
            RunMode::Copy
        } else {
            RunMode::Stream
        },
        created_at: r.get(7)?,
        updated_at: r.get(8)?,
    })
}

pub fn list(tx: &Transaction) -> AppResult<Vec<Script>> {
    let mut stmt = tx.prepare(&format!(
        "SELECT {COLUMNS} FROM scripts ORDER BY name COLLATE NOCASE"
    ))?;
    let rows = stmt.query_map([], from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn get(tx: &Transaction, id: &str) -> AppResult<Script> {
    tx.query_row(
        &format!("SELECT {COLUMNS} FROM scripts WHERE id = ?1"),
        [id],
        from_row,
    )
    .optional()?
    .ok_or_else(|| AppError::NotFound(format!("script {id}")))
}

pub fn check_interpreter(i: &str) -> AppResult<()> {
    let ok = !i.is_empty()
        && i.len() <= 32
        && i.starts_with(|c: char| c.is_ascii_lowercase())
        && i.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "._-".contains(c));
    if !ok {
        return Err(AppError::Validation(format!(
            "invalid interpreter {i:?} (e.g. bash, sh, python3)"
        )));
    }
    Ok(())
}

/// LF line endings (scripts written on Windows otherwise fail with `$'\r': command not found`),
/// no BOM, trailing newline.
pub fn normalize(content: &str) -> String {
    let s = content.strip_prefix('\u{feff}').unwrap_or(content);
    let mut s = s.replace("\r\n", "\n").replace('\r', "\n");
    if !s.ends_with('\n') {
        s.push('\n');
    }
    s
}

fn validate(input: &mut ScriptInput) -> AppResult<()> {
    input.name = input.name.trim().to_string();
    if input.name.is_empty() || input.name.chars().count() > 100 {
        return Err(AppError::Validation(
            "the script name must be 1–100 characters".into(),
        ));
    }
    input.interpreter = input.interpreter.trim().to_string();
    check_interpreter(&input.interpreter)?;
    input.content = normalize(&input.content);
    if input.content.trim().is_empty() {
        return Err(AppError::Validation("the script is empty".into()));
    }
    if input.content.len() > MAX_SCRIPT_BYTES {
        return Err(AppError::Validation(format!(
            "scripts can be at most {} KB",
            MAX_SCRIPT_BYTES / 1000
        )));
    }
    input.default_args = input.default_args.trim().to_string();
    Ok(())
}

pub fn create(tx: &Transaction, input: ScriptInput, source: Source) -> AppResult<Script> {
    create_with_id(tx, &new_id(), input, source)
}

/// Creates a script with a given id (used when undoing its deletion).
pub fn create_with_id(
    tx: &Transaction,
    id: &str,
    mut input: ScriptInput,
    source: Source,
) -> AppResult<Script> {
    validate(&mut input)?;
    let ts = now();
    tx.execute(
        &format!("INSERT INTO scripts ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)"),
        params![
            id,
            input.name,
            input.description,
            input.content,
            input.interpreter,
            input.default_args,
            input.run_mode.as_str(),
            ts
        ],
    )?;
    let s = get(tx, id)?;
    history::record(
        tx,
        source,
        EntityType::Script,
        id,
        Action::Create,
        None::<&Script>,
        Some(&s),
    )?;
    Ok(s)
}

pub fn update(
    tx: &Transaction,
    id: &str,
    mut input: ScriptInput,
    source: Source,
) -> AppResult<Script> {
    validate(&mut input)?;
    let before = get(tx, id)?;
    tx.execute(
        "UPDATE scripts SET name = ?2, description = ?3, content = ?4, interpreter = ?5,
            default_args = ?6, run_mode = ?7, updated_at = ?8 WHERE id = ?1",
        params![
            id,
            input.name,
            input.description,
            input.content,
            input.interpreter,
            input.default_args,
            input.run_mode.as_str(),
            now()
        ],
    )?;
    let after = get(tx, id)?;
    if after != before {
        history::record(
            tx,
            source,
            EntityType::Script,
            id,
            Action::Update,
            Some(&before),
            Some(&after),
        )?;
    }
    Ok(after)
}

pub fn delete(tx: &Transaction, id: &str, source: Source) -> AppResult<()> {
    let before = get(tx, id)?;
    tx.execute("DELETE FROM scripts WHERE id = ?1", [id])?;
    history::record(
        tx,
        source,
        EntityType::Script,
        id,
        Action::Delete,
        Some(&before),
        None::<&Script>,
    )
}

/// The remote command (one argv element after the destination) that runs `content`.
/// `args` is parsed like a shell command line (quotes group words) and every word is passed
/// as one quoted argument.
pub fn remote_command(
    content: &str,
    interpreter: &str,
    mode: RunMode,
    args: &str,
) -> AppResult<String> {
    check_interpreter(interpreter)?;
    let script = normalize(content);
    if script.len() > MAX_SCRIPT_BYTES {
        return Err(AppError::Validation(format!(
            "scripts can be at most {} KB",
            MAX_SCRIPT_BYTES / 1000
        )));
    }
    if args.chars().any(char::is_control) {
        return Err(AppError::Validation(
            "arguments cannot contain control characters".into(),
        ));
    }
    let b64 = base64::engine::general_purpose::STANDARD.encode(script.as_bytes());
    let args: String = split_args(args)
        .iter()
        .map(|a| format!(" {}", posix_quote(a)))
        .collect();
    let inner = match mode {
        RunMode::Stream => format!("printf %s '{b64}' | base64 -d | {interpreter} -s --{args}"),
        RunMode::Copy => format!(
            "f=$(mktemp) || exit 1; printf %s '{b64}' | base64 -d > \"$f\" && {interpreter} \"$f\"{args}; rc=$?; rm -f \"$f\"; exit $rc"
        ),
    };
    Ok(format!("sh -c {}", posix_quote(&inner)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn input(name: &str, content: &str) -> ScriptInput {
        ScriptInput {
            name: name.into(),
            description: String::new(),
            content: content.into(),
            interpreter: "bash".into(),
            default_args: String::new(),
            run_mode: RunMode::Stream,
        }
    }

    #[test]
    fn crud_normalizes_and_records_history() {
        let db = Db::open_in_memory().unwrap();
        let s = db
            .tx(|tx| create(tx, input(" Disk ", "\u{feff}df -h\r\nuptime"), Source::User))
            .unwrap();
        assert_eq!(s.name, "Disk");
        assert_eq!(s.content, "df -h\nuptime\n");
        let mut i = input("Disk", "df -h");
        i.run_mode = RunMode::Copy;
        let s = db.tx(|tx| update(tx, &s.id, i, Source::User)).unwrap();
        assert_eq!(s.run_mode, RunMode::Copy);
        assert_eq!(db.tx(list).unwrap().len(), 1);
        db.tx(|tx| delete(tx, &s.id, Source::User)).unwrap();
        assert!(db.tx(list).unwrap().is_empty());
        let log = db
            .tx(|tx| history::for_entity(tx, EntityType::Script, &s.id, 10))
            .unwrap();
        assert_eq!(log.len(), 3);

        assert!(
            db.tx(|tx| create(tx, input("", "x"), Source::User))
                .is_err()
        );
        assert!(
            db.tx(|tx| create(tx, input("x", "  \n"), Source::User))
                .is_err()
        );
        let mut bad = input("x", "ls");
        bad.interpreter = "bash; rm".into();
        assert!(db.tx(|tx| create(tx, bad, Source::User)).is_err());
        assert!(
            db.tx(|tx| create(
                tx,
                input("x", &"a".repeat(MAX_SCRIPT_BYTES + 1)),
                Source::User
            ))
            .is_err()
        );
    }

    #[test]
    fn remote_command_shapes() {
        let cmd = remote_command(
            "echo hi\r\n",
            "bash",
            RunMode::Stream,
            "one 'two three' it's",
        )
        .unwrap();
        // "echo hi\n" in base64.
        assert!(
            cmd.starts_with("sh -c 'printf %s '\\''ZWNobyBoaQo='\\'' | base64 -d | bash -s -- "),
            "{cmd}"
        );
        assert!(cmd.contains(r"'\''one'\'' '\''two three'\''"), "{cmd}");
        let copy = remote_command("ls", "sh", RunMode::Copy, "").unwrap();
        assert!(copy.contains("mktemp") && copy.contains("rm -f"), "{copy}");
        assert!(remote_command("ls", "bash -c", RunMode::Stream, "").is_err());
    }

    /// Runs the generated command with a local POSIX shell (Git's sh on Windows) when there is
    /// one, proving the quoting and both modes end to end.
    #[test]
    fn generated_commands_run_in_a_real_shell() {
        let Ok(sh) = which::which("sh") else {
            eprintln!("no sh; skipping");
            return;
        };
        let script = "printf '%s|' \"$@\"\necho \"n=$#\"\n";
        for mode in [RunMode::Stream, RunMode::Copy] {
            let cmd = remote_command(script, "sh", mode, "a 'b c' \"it's\" '$HOME'").unwrap();
            // A real shell parses the whole command, like the remote login shell does.
            let out = std::process::Command::new(&sh)
                .arg("-c")
                .arg(&cmd)
                .output()
                .unwrap();
            let stdout = String::from_utf8_lossy(&out.stdout);
            assert!(
                out.status.success(),
                "{mode:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            assert_eq!(stdout.trim(), "a|b c|it's|$HOME|n=4", "{mode:?}");
        }
    }
}
