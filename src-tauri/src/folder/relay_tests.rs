//! Phase 4: streaming between servers, against a stand-in "ssh" that runs the command with
//! the local shell. It runs wherever `sh` does (Git Bash on Windows too), so the pipes,
//! kills and waits are tested on every platform.

use std::path::PathBuf;
use std::process::Command;

use super::relay::{Relayed, fetch_manifest, probe, relay};
use super::runner::Conn;
use super::{remote_archive, remote_inbox};

/// Called as `sh ssh.sh <options> <destination> <command>`. The destination "nokey" refuses
/// BatchMode logins, like a server that only takes passwords.
const STAND_IN: &str = r#"batch=
for a; do
  [ "$a" = BatchMode=yes ] && batch=1
  [ "$a" = nokey ] && [ -n "$batch" ] && { echo 'Permission denied (publickey,password).' >&2; exit 255; }
  last=$a
done
exec sh -c "$last"
"#;

struct Fake {
    _dir: tempfile::TempDir,
    sh: PathBuf,
    script: String,
}

impl Fake {
    fn new() -> Option<Self> {
        let sh = which::which("sh").ok()?;
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("ssh.sh");
        std::fs::write(&script, STAND_IN).unwrap();
        Some(Self {
            script: script.display().to_string().replace('\\', "/"),
            _dir: dir,
            sh,
        })
    }

    fn conn(&self, name: &str) -> Conn {
        Conn {
            options: vec![self.script.clone()],
            destination: name.into(),
        }
    }

    fn sh(&self, cmd: &str) -> String {
        let out = Command::new(&self.sh).arg("-c").arg(cmd).output().unwrap();
        assert!(out.status.success(), "{cmd}: {out:?}");
        String::from_utf8_lossy(&out.stdout).into_owned()
    }
}

#[test]
fn streams_an_archive_and_explains_failures() {
    let Some(f) = Fake::new() else { return };
    let id = uuid::Uuid::new_v4().simple().to_string()[..12].to_string();
    let (archive, inbox) = (remote_archive(&id), remote_inbox(&id));
    // About 3 MB: more than one buffer, and more than a pipe holds.
    f.sh(&format!(
        r#"i=0; while [ $i -lt 3000 ]; do printf '%01023d\n' $i; i=$((i+1)); done > {archive}
printf '%s %s site\n' - $(wc -c < {archive} | tr -d ' ') > {archive}.sum"#
    ));
    let (from, to) = (f.conn("src"), f.conn("dst"));

    let m = fetch_manifest(&f.sh, &from, &archive).unwrap();
    assert_eq!((m.bytes, m.name.as_str()), (3000 * 1024, "site"));
    assert_eq!(m.sha256, None);
    probe(&f.sh, &to).unwrap();

    let mut seen = Vec::new();
    let r = relay(&f.sh, &from, &archive, &to, &inbox, &|| false, &mut |b| {
        seen.push(b)
    });
    let Relayed::Done { bytes, sha256 } = r.unwrap() else {
        panic!("not cancelled")
    };
    assert_eq!(bytes, m.bytes);
    assert_eq!(seen.last(), Some(&bytes));
    assert_eq!(
        sha256,
        crate::folder::sha256_file(&local(&f, &archive)).unwrap()
    );
    assert!(
        f.sh(&format!("cmp {archive} {inbox} && echo same"))
            .contains("same")
    );

    // A server that needs a password.
    let err = probe(&f.sh, &f.conn("nokey")).unwrap_err().to_string();
    assert!(err.contains("without a prompt"), "{err}");
    assert!(err.contains("Permission denied"), "{err}");

    // No archive on the source.
    let err = relay(
        &f.sh,
        &from,
        &remote_archive("ffffffffffff"),
        &to,
        &inbox,
        &|| false,
        &mut |_| {},
    )
    .err()
    .expect("nothing to read")
    .to_string();
    assert!(err.contains("reading the archive on src failed"), "{err}");

    // The destination cannot save it (a folder is in the way).
    f.sh(&format!("rm -f {inbox} && mkdir {inbox}"));
    let err = relay(&f.sh, &from, &archive, &to, &inbox, &|| false, &mut |_| {})
        .err()
        .expect("cannot save")
        .to_string();
    assert!(err.contains("saving the archive on dst failed"), "{err}");

    let r = relay(&f.sh, &from, &archive, &to, &inbox, &|| true, &mut |_| {}).unwrap();
    assert!(matches!(r, Relayed::Cancelled));

    f.sh(&format!("rm -rf {archive} {archive}.sum {inbox}"));
}

/// The shell's `/tmp/…` as a path this process can open (Git Bash maps it elsewhere).
fn local(f: &Fake, path: &str) -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(f.sh(&format!("cygpath -w {path}")).trim())
    } else {
        PathBuf::from(path)
    }
}
