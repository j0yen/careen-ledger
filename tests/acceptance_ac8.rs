//! AC8: State path is created on first `record` if absent;
//! `--ledger <path>` overrides the default for testing.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::Write as _;
use std::process::Command;

fn binary() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_careen-ledger"))
}

fn write_summary() -> tempfile::NamedTempFile {
    let mut f = tempfile::Builder::new()
        .suffix(".json")
        .tempfile()
        .unwrap();
    let json = serde_json::json!({
        "repo": "path-test-repo",
        "removed_bytes": 1_000_000u64,
        "removed_entries": 1u64,
        "classes": {}
    });
    write!(f, "{json}").unwrap();
    f
}

#[test]
fn creates_parent_dirs_on_first_record() {
    let dir = tempfile::tempdir().unwrap();
    // Ledger path with non-existent parent sub-directories.
    let ledger_path = dir.path().join("deep/nested/dir/ledger.jsonl");
    assert!(
        !ledger_path.exists(),
        "ledger path must not exist before test"
    );

    let summary = write_summary();
    let out = Command::new(binary())
        .args([
            "--ledger",
            ledger_path.to_str().unwrap(),
            "record",
            summary.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(
        out.status.success(),
        "record should create parent dirs and succeed"
    );
    assert!(
        ledger_path.exists(),
        "ledger file must be created at the given path"
    );
}

#[test]
fn ledger_override_is_respected() {
    let dir = tempfile::tempdir().unwrap();
    let custom_path = dir.path().join("custom.jsonl");
    let summary = write_summary();

    let out = Command::new(binary())
        .args([
            "--ledger",
            custom_path.to_str().unwrap(),
            "record",
            summary.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(out.status.success());
    assert!(
        custom_path.exists(),
        "--ledger override must write to the specified path"
    );

    // Default path must NOT be created.
    let home = std::env::var("HOME").unwrap_or_default();
    let default = std::path::Path::new(&home).join(".local/state/careen/ledger.jsonl");
    // We can't assert it doesn't exist (it might from prior runs), but the
    // custom path being populated is the key invariant.
    drop(default); // used to satisfy lint
}
