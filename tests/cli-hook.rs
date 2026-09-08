// Integration tests for the Claude Code hook subcommands: the hidden internal
// hook-callback entry point and hook install.
//
// Every test runs against its own tempdir through the environment the code
// honors: XDG_CACHE_HOME for the duplicate-suppression cache, XDG_CONFIG_HOME
// for the override store, CLAUDE_CONFIG_DIR for settings.json. Nothing here may
// read or write the developer's real home directory.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_zhtw-mcp");

/// Run `internal hook-callback` with `payload` on stdin and both state
/// directories pointed into `dir`.
fn run_callback(payload: &str, dir: &Path) -> Output {
    Command::new(BIN)
        .args(["internal", "hook-callback"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_remove("RUST_LOG")
        .env("XDG_CACHE_HOME", dir.join("cache"))
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .spawn()
        .and_then(|mut child| {
            child
                .stdin
                .take()
                .unwrap()
                .write_all(payload.as_bytes())
                .unwrap();
            child.wait_with_output()
        })
        .unwrap()
}

fn run_install(dir: &Path) -> Output {
    Command::new(BIN)
        .args(["hook", "install"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_remove("RUST_LOG")
        .env("CLAUDE_CONFIG_DIR", dir.join("claude"))
        .output()
        .unwrap()
}

fn write_payload(tool: &str, file: &Path) -> String {
    serde_json::json!({
        "session_id": "test",
        "hook_event_name": "PostToolUse",
        "tool_name": tool,
        "tool_input": { "file_path": file.to_string_lossy() },
        "tool_response": {},
    })
    .to_string()
}

/// Exit 0 with empty stdout and stderr: the shape of every suppressed path.
fn assert_silent(output: &Output, what: &str) {
    assert!(output.status.success(), "{what}: expected exit 0");
    assert!(
        output.stdout.is_empty(),
        "{what}: expected empty stdout, got: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        output.stderr.is_empty(),
        "{what}: expected empty stderr, got: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A tempdir with an empty .git inside, which is where the project-config
/// walk stops: without it, a stray .zhtw-mcp.toml above the temp root would
/// leak into the scan and break the hermetic-environment contract at the top
/// of this file.
fn hermetic_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".git")).unwrap();
    dir
}

/// The total the summary line claims: "zhtw-mcp: N zh-TW issue(s) in ...".
fn reported_total(context: &str) -> usize {
    context
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("zhtw-mcp: "))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|n| n.parse().ok())
        .unwrap_or_else(|| panic!("summary line should carry a count: {context}"))
}

fn additional_context(output: &Output) -> String {
    let value: serde_json::Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|e| panic!("stdout should be JSON: {e}"));
    assert_eq!(value["hookSpecificOutput"]["hookEventName"], "PostToolUse");
    value["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .expect("additionalContext should be a string")
        .to_string()
}

#[test]
fn callback_is_silent_on_everything_that_is_not_a_lintable_write() {
    let dir = hermetic_dir();
    let md = dir.path().join("a.md");
    std::fs::write(&md, "這個軟件很好\n").unwrap();

    for (what, payload) in [
        ("garbage stdin", "not json at all".to_string()),
        ("read tool", write_payload("Read", &md)),
        (
            "rust file",
            write_payload("Write", &dir.path().join("a.rs")),
        ),
        (
            "missing file",
            write_payload("Write", &dir.path().join("gone.md")),
        ),
    ] {
        assert_silent(&run_callback(&payload, dir.path()), what);
    }
}

#[test]
fn callback_is_silent_on_a_clean_file() {
    let dir = hermetic_dir();
    let md = dir.path().join("clean.md");
    std::fs::write(&md, "# Title\n\nEnglish only, nothing to report.\n").unwrap();
    assert_silent(
        &run_callback(&write_payload("Write", &md), dir.path()),
        "clean file",
    );
}

#[test]
fn callback_reports_issues_once_and_caps_the_list() {
    let dir = hermetic_dir();
    let md = dir.path().join("guide.md");
    std::fs::write(
        &md,
        "# 說明\n\n這個軟件的默認設置很好。質量不錯，網絡也穩定，內存充足。\n",
    )
    .unwrap();

    // First write: a capped report reaches stdout. Every count below is derived
    // from the total the summary line itself claims, because the exact tally
    // for this sample belongs to the ruleset inventory and moves with it; the
    // mechanism under test is the cap and the tail, not the inventory.
    let output = run_callback(&write_payload("Write", &md), dir.path());
    assert!(output.status.success());
    let context = additional_context(&output);
    let total = reported_total(&context);
    assert!(context.contains("zh-TW issue(s) in guide.md"), "{context}");
    assert!(context.contains("軟件 → 軟體"), "{context}");
    assert!(
        total > 3,
        "the fixture exists to exercise the tail; give it more distinct \
         zh-CN terms: {context}"
    );
    let issue_lines = context.lines().filter(|l| l.contains('→')).count();
    assert_eq!(issue_lines, 3, "cap at three issue lines: {context}");
    let tail = format!("+{} more: zhtw-mcp lint", total - 3);
    assert!(context.contains(&tail), "{context}");
    assert!(
        context.contains(&md.to_string_lossy().into_owned()),
        "the tail carries the full path for a follow-up lint: {context}"
    );

    // Same content again (an Edit this time): suppressed by content hash.
    assert_silent(
        &run_callback(&write_payload("Edit", &md), dir.path()),
        "unchanged content",
    );

    // Content changed elsewhere, issue set identical: still suppressed.
    std::fs::write(
        &md,
        "# 說明\n\nA clean English paragraph.\n\n這個軟件的默認設置很好。質量不錯，網絡也穩定，內存充足。\n",
    )
    .unwrap();
    assert_silent(
        &run_callback(&write_payload("Edit", &md), dir.path()),
        "unchanged issue set",
    );

    // A new issue appears: the report comes back.
    std::fs::write(
        &md,
        "# 說明\n\n這個軟件的默認設置很好。質量不錯，網絡也穩定，內存充足。鼠標也好用。\n",
    )
    .unwrap();
    let output = run_callback(&write_payload("Edit", &md), dir.path());
    let context = additional_context(&output);
    assert!(context.contains("zh-TW issue(s)"), "{context}");
}

#[test]
fn callback_becomes_silent_after_the_issues_are_fixed() {
    let dir = hermetic_dir();
    let md = dir.path().join("fixed.md");
    std::fs::write(&md, "這個軟件很好\n").unwrap();
    let output = run_callback(&write_payload("Write", &md), dir.path());
    assert!(!output.stdout.is_empty(), "first pass reports");

    std::fs::write(&md, "這個軟體很好\n").unwrap();
    assert_silent(
        &run_callback(&write_payload("Edit", &md), dir.path()),
        "fixed file",
    );
}

#[test]
fn callback_converts_simplified_input_before_scanning() {
    let dir = hermetic_dir();
    let md = dir.path().join("sc.md");
    std::fs::write(&md, "这个软件的默认设置很好。\n").unwrap();

    // An agent writing Simplified Chinese is the case the hook exists to catch:
    // the S2T pass turns 软件 into 軟件, which the ruleset then flags.
    let output = run_callback(&write_payload("Write", &md), dir.path());
    let context = additional_context(&output);
    assert!(context.contains("軟件 → 軟體"), "{context}");
    assert!(context.contains("默認 → 預設"), "{context}");
}

#[test]
fn callback_honors_the_project_glossary() {
    let dir = hermetic_dir();
    std::fs::write(
        dir.path().join(".zhtw-mcp.toml"),
        "[glossary]\nproper_nouns = [\"軟件\"]\n",
    )
    .unwrap();
    let md = dir.path().join("doc.md");
    std::fs::write(&md, "這個軟件很好\n").unwrap();

    // The glossary declares the term a proper noun, so the lint front end keeps
    // quiet about it and the hook has to as well.
    assert_silent(
        &run_callback(&write_payload("Write", &md), dir.path()),
        "glossary proper noun",
    );
}

#[test]
fn callback_skips_non_regular_files() {
    let dir = hermetic_dir();

    let fake_dir = dir.path().join("dir.md");
    std::fs::create_dir(&fake_dir).unwrap();
    assert_silent(
        &run_callback(&write_payload("Write", &fake_dir), dir.path()),
        "directory named like markdown",
    );

    // A FIFO stats as zero length and blocks at open; the callback must reject
    // it on the stat and return, not hang on the agent's write path.
    #[cfg(unix)]
    {
        let fifo = dir.path().join("pipe.md");
        let status = Command::new("mkfifo").arg(&fifo).status().unwrap();
        assert!(status.success(), "mkfifo should succeed");
        assert_silent(
            &run_callback(&write_payload("Write", &fifo), dir.path()),
            "fifo named like markdown",
        );
    }
}

#[cfg(unix)]
#[test]
fn install_follows_a_symlinked_settings_file() {
    let dir = tempfile::tempdir().unwrap();
    let claude_dir = dir.path().join("claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    let real = dir.path().join("dotfiles").join("settings.json");
    std::fs::create_dir_all(real.parent().unwrap()).unwrap();
    std::fs::write(&real, "{}\n").unwrap();
    let link = claude_dir.join("settings.json");
    std::os::unix::fs::symlink(&real, &link).unwrap();

    assert!(run_install(dir.path()).status.success());

    // The atomic write must land in the dotfiles target, not replace the link
    // with a regular file the next dotfiles sync would clobber.
    let meta = std::fs::symlink_metadata(&link).unwrap();
    assert!(meta.file_type().is_symlink(), "the link must survive");
    let root: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&real).unwrap()).unwrap();
    assert_eq!(root["hooks"]["PostToolUse"][0]["matcher"], "Write|Edit");
}

#[test]
fn install_creates_settings_and_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let settings = dir.path().join("claude").join("settings.json");

    let output = run_install(dir.path());
    assert!(output.status.success());
    assert!(output.stdout.is_empty(), "install writes no stdout");
    assert!(String::from_utf8_lossy(&output.stderr).contains("installed"));

    let text = std::fs::read_to_string(&settings).unwrap();
    let root: serde_json::Value = serde_json::from_str(&text).unwrap();
    let entry = &root["hooks"]["PostToolUse"][0];
    assert_eq!(entry["matcher"], "Write|Edit");
    let command = entry["hooks"][0]["command"].as_str().unwrap();
    assert!(command.ends_with("internal hook-callback"), "{command}");
    assert!(command.contains("zhtw-mcp"), "{command}");

    let output = run_install(dir.path());
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("already installed"));
    assert_eq!(
        std::fs::read_to_string(&settings).unwrap(),
        text,
        "second install must not change the file"
    );
}

#[test]
fn install_merges_into_existing_settings_without_touching_them() {
    let dir = tempfile::tempdir().unwrap();
    let claude_dir = dir.path().join("claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    let settings = claude_dir.join("settings.json");
    std::fs::write(
        &settings,
        r#"{
          "model": "opus",
          "hooks": {
            "PostToolUse": [
              {"matcher": "Bash", "hooks": [{"type": "command", "command": "other-tool"}]}
            ]
          }
        }"#,
    )
    .unwrap();

    assert!(run_install(dir.path()).status.success());
    let root: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
    assert_eq!(root["model"], "opus", "unrelated settings survive");
    let post = root["hooks"]["PostToolUse"].as_array().unwrap();
    assert_eq!(post.len(), 2);
    assert_eq!(post[0]["hooks"][0]["command"], "other-tool");
    assert_eq!(post[1]["matcher"], "Write|Edit");
}

#[test]
fn install_refuses_to_clobber_an_unparseable_settings_file() {
    let dir = tempfile::tempdir().unwrap();
    let claude_dir = dir.path().join("claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    let settings = claude_dir.join("settings.json");
    std::fs::write(&settings, "{not json").unwrap();

    let output = run_install(dir.path());
    assert_eq!(
        output.status.code(),
        Some(2),
        "broken input is a hard error"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("refusing to modify"));
    assert_eq!(
        std::fs::read_to_string(&settings).unwrap(),
        "{not json",
        "the broken file must be left exactly as found"
    );
}
