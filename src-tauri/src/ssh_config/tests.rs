use super::*;

const SAMPLE: &str = "\
# Global settings
ServerAliveInterval 60
Include conf.d/*.conf

Host bastion
    HostName bastion.example.com
    User ops
    IdentityFile ~/.ssh/ops_ed25519

# Database behind the bastion
Host prod-db
    HostName=10.0.0.5
    User deploy
    Port 2222
    ProxyJump bastion
    IdentityFile ~/.ssh/deploy
    IdentityFile ~/.ssh/deploy_old
    LocalForward 5433 localhost:5432
    IdentitiesOnly yes

Host web-* *.internal
    User www

Match host *.corp exec \"test -f ~/.vpn\"
    ProxyJump bastion

Host *
    ServerAliveCountMax 3
";

fn parse(text: &str) -> ParsedConfig {
    let file = ConfigFile::parse("config", text);
    let mut out = ParsedConfig::default();
    for block in &file.blocks {
        collect_block(&file, block, &mut out);
    }
    out.files.push(file);
    out
}

#[test]
fn round_trips_exactly() {
    for text in [
        SAMPLE,
        "",
        "\n",
        "Host a",
        "Host a\r\n  HostName x\r\n",
        "Host a\n  HostName x\r\n\r\n# mixed\n",
        "\u{feff}Host a\n",
        "Host a\n\n\n",
        "  \t# indented comment\nHost\ta\n\tUser = \"me\"  \n",
        "Host a\r",
    ] {
        let file = ConfigFile::parse("config", text);
        assert_eq!(file.render(), text, "round trip failed for {text:?}");
    }
}

#[test]
fn detects_dominant_line_ending() {
    assert_eq!(
        ConfigFile::parse("c", "a\r\nb\r\nc\n").line_ending,
        LineEnding::CrLf
    );
    assert_eq!(ConfigFile::parse("c", "a\nb\n").line_ending, LineEnding::Lf);
}

#[test]
fn splits_blocks() {
    let file = ConfigFile::parse("config", SAMPLE);
    let kinds: Vec<_> = file
        .blocks
        .iter()
        .map(|b| match &b.kind {
            BlockKind::Preamble => "preamble".to_string(),
            BlockKind::Host { patterns } => format!("host:{}", patterns.join(",")),
            BlockKind::Match { .. } => "match".to_string(),
        })
        .collect();
    assert_eq!(
        kinds,
        [
            "preamble",
            "host:bastion",
            "host:prod-db",
            "host:web-*,*.internal",
            "match",
            "host:*"
        ]
    );
    // The comment above `Host prod-db` belongs to the bastion block's tail.
    assert_eq!(file.blocks[1].end, file.blocks[2].start);
}

#[test]
fn extracts_managed_hosts() {
    let p = parse(SAMPLE);
    let aliases: Vec<_> = p.hosts.iter().map(|h| h.alias.as_str()).collect();
    assert_eq!(aliases, ["bastion", "prod-db"]);

    let db = &p.hosts[1];
    assert_eq!(db.hostname.as_deref(), Some("10.0.0.5"));
    assert_eq!(db.user.as_deref(), Some("deploy"));
    assert_eq!(db.port, Some(2222));
    assert_eq!(db.proxy_jump.as_deref(), Some("bastion"));
    assert_eq!(db.identity_file.as_deref(), Some("~/.ssh/deploy"));
    assert_eq!(db.line, 11);
    let extra: Vec<_> = db
        .extra_options
        .iter()
        .map(|d| (d.key.as_str(), d.value.as_str()))
        .collect();
    assert_eq!(
        extra,
        [
            ("IdentityFile", "~/.ssh/deploy_old"),
            ("LocalForward", "5433 localhost:5432"),
            ("IdentitiesOnly", "yes"),
        ]
    );
}

#[test]
fn wildcards_and_match_are_read_only_rules() {
    let p = parse(SAMPLE);
    let rules: Vec<_> = p
        .rules
        .iter()
        .map(|r| (r.kind, r.header.as_str(), r.line))
        .collect();
    assert_eq!(
        rules,
        [
            ("host", "web-* *.internal", 21),
            ("match", "host *.corp exec \"test -f ~/.vpn\"", 24),
            ("host", "*", 27),
        ]
    );
}

#[test]
fn duplicate_alias_keeps_first_block() {
    let p = parse("Host a\n  Port 1\nHost a\n  Port 2\n");
    assert_eq!(p.hosts.len(), 1);
    assert_eq!(p.hosts[0].port, Some(1));
    assert_eq!(p.rules.len(), 1);
    assert!(p.warnings[0].contains("defined again"));
}

#[test]
fn invalid_port_is_kept_as_option() {
    let p = parse("Host a\n  Port abc\n");
    assert_eq!(p.hosts[0].port, None);
    assert_eq!(p.hosts[0].extra_options[0].key, "Port");
    assert_eq!(p.warnings.len(), 1);
}

#[test]
fn quoted_values_are_unquoted_for_main_fields() {
    let p = parse("Host a\n  IdentityFile \"C:\\Users\\me\\.ssh\\id rsa\"\n");
    assert_eq!(
        p.hosts[0].identity_file.as_deref(),
        Some("C:\\Users\\me\\.ssh\\id rsa")
    );
}

#[test]
fn fingerprint_tracks_directive_changes_only() {
    let fp = |t: &str| parse(t).hosts[0].fingerprint.clone();
    let base = fp("Host a\n  HostName x\n");
    assert_eq!(base, fp("Host a\n\n    hostname x   \n# comment\n"));
    assert_ne!(base, fp("Host a\n  HostName y\n"));
}

#[test]
fn follows_includes() {
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("config");
    std::fs::create_dir(dir.path().join("conf.d")).unwrap();
    std::fs::write(
        &main,
        "Include conf.d/*.conf\nInclude missing.conf\nHost main\n  HostName m\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("conf.d/a.conf"),
        "Host inc-a\n  HostName a\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("conf.d/b.conf"),
        "Include config\nHost inc-b\n",
    )
    .unwrap();

    let p = load(&main).unwrap();
    let mut aliases: Vec<_> = p.hosts.iter().map(|h| h.alias.as_str()).collect();
    aliases.sort();
    assert_eq!(aliases, ["inc-a", "inc-b", "main"]);
    assert_eq!(p.files.len(), 3);
    assert!(
        p.hosts
            .iter()
            .find(|h| h.alias == "inc-a")
            .unwrap()
            .source
            .ends_with("a.conf")
    );
    assert!(p.warnings.iter().any(|w| w.contains("missing.conf")));
    assert!(p.warnings.iter().any(|w| w.contains("more than once")));
}

#[test]
fn missing_config_is_empty() {
    let dir = tempfile::tempdir().unwrap();
    let p = load(&dir.path().join("config")).unwrap();
    assert!(p.hosts.is_empty());
    assert_eq!(p.warnings.len(), 1);
}

#[test]
fn rejects_utf16() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config");
    std::fs::write(&path, [0xff, 0xfe, b'H', 0]).unwrap();
    assert!(matches!(load(&path), Err(AppError::Validation(_))));
}

/// Checks the real `~/.ssh/config` round-trips. Prints only counts, never contents.
/// Run with `cargo test real_config -- --ignored --nocapture`.
#[test]
#[ignore]
fn real_config_round_trips() {
    let path = crate::paths::default_ssh_config().unwrap();
    let p = load(&path).unwrap();
    for f in &p.files {
        let original = std::fs::read_to_string(&f.path).unwrap();
        assert!(
            f.render() == original,
            "round trip differs for {}",
            f.path.display()
        );
    }
    println!(
        "files={} managed_hosts={} rules={} warnings={}",
        p.files.len(),
        p.hosts.len(),
        p.rules.len(),
        p.warnings.len()
    );
    for w in &p.warnings {
        println!("warning: {}", w.split(':').next().unwrap_or(""));
    }
}

#[test]
fn per_user_match_blocks_become_variants() {
    let p = parse(
        "Host devbox\n    HostName 203.0.113.10\n    IdentitiesOnly yes\n\n\
         Match originalhost devbox user deploy\n    IdentityFile \"C:\\keys\\dev deploy\"\n\n\
         Match user tom originalhost devbox\n    IdentityFile c:\\keys\\devbox\n    ForwardAgent yes\n\n\
         Match host devbox user root\n    IdentityFile x\n\
         Match originalhost devbox user tom exec \"true\"\n    IdentityFile y\n\
         Match originalhost dev* user root\n    IdentityFile z\n",
    );
    let v: Vec<_> = p
        .variants
        .iter()
        .map(|v| {
            (
                v.alias.as_str(),
                v.user.as_str(),
                v.identity_file.as_deref(),
                v.options.len(),
            )
        })
        .collect();
    assert_eq!(
        v,
        [
            ("devbox", "deploy", Some(r"C:\keys\dev deploy"), 1),
            ("devbox", "tom", Some(r"c:\keys\devbox"), 2),
        ]
    );
    assert_eq!(p.variants[1].line, 8);
    // Still listed as read-only rules, and the file is untouched.
    assert_eq!(p.rules.len(), 5);
}

/// Lists the per-user Match blocks of the developer's real config (read-only).
#[test]
#[ignore = "reads ~/.ssh/config"]
fn real_config_variants() {
    let path = dirs::home_dir().unwrap().join(".ssh").join("config");
    let p = super::load(&path).unwrap();
    for v in &p.variants {
        let managed = p.hosts.iter().any(|h| h.alias == v.alias);
        println!(
            "{}@{} key={:?} managed={managed}",
            v.user, v.alias, v.identity_file
        );
    }
    println!("{} variants, {} rules", p.variants.len(), p.rules.len());
}
