//! AC6: The ledger never rewrites or deletes a prior line; corruption of one
//! line (bad JSON) is skipped with a warning on read, not a crash.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::Write as _;
use std::process::Command;

fn binary() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_careen-ledger"))
}

fn write_summary(repo: &str) -> tempfile::NamedTempFile {
    let mut f = tempfile::Builder::new()
        .suffix(".json")
        .tempfile()
        .unwrap();
    let json = serde_json::json!({
        "repo": repo,
        "removed_bytes": 3_000_000u64,
        "removed_entries": 3u64,
        "classes": {}
    });
    write!(f, "{json}").unwrap();
    f
}

#[test]
fn corrupt_line_is_skipped_not_crash() {
    let ledger = tempfile::Builder::new()
        .suffix(".jsonl")
        .tempfile()
        .unwrap();

    // Prepopulate with a valid line + a corrupt line.
    {
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(ledger.path())
            .unwrap();
        writeln!(
            f,
            r#"{{"kind":"sweep","id":"abc","ts":"2026-01-01T00:00:00Z","repo":"old-repo","removed_bytes":1000,"removed_entries":1}}"#
        )
        .unwrap();
        writeln!(f, "{{this is not valid json{{").unwrap();
    }

    // Now record a new sweep — should succeed despite the corrupt line.
    let summary = write_summary("new-repo");
    let out = Command::new(binary())
        .args([
            "--ledger",
            ledger.path().to_str().unwrap(),
            "record",
            summary.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(out.status.success(), "record should succeed despite corrupt line");

    // Verdict on old-repo should also succeed (read path must skip corrupt line).
    let out2 = Command::new(binary())
        .args([
            "--ledger",
            ledger.path().to_str().unwrap(),
            "verdict",
            "old-repo",
        ])
        .output()
        .unwrap();
    assert!(out2.status.success(), "verdict should succeed despite corrupt line");

    // The corrupt line warning should appear on stderr.
    let stderr = String::from_utf8(out2.stderr).unwrap();
    assert!(
        stderr.contains("warning") || stderr.contains("skipping"),
        "expected a warning on stderr about corrupt line, got: {stderr}"
    );
}

#[test]
fn ledger_is_append_only_no_deletion() {
    let ledger = tempfile::Builder::new()
        .suffix(".jsonl")
        .tempfile()
        .unwrap();
    let summary1 = write_summary("repo-x");
    let summary2 = write_summary("repo-y");

    Command::new(binary())
        .args([
            "--ledger",
            ledger.path().to_str().unwrap(),
            "record",
            summary1.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    let size1 = std::fs::metadata(ledger.path()).unwrap().len();

    Command::new(binary())
        .args([
            "--ledger",
            ledger.path().to_str().unwrap(),
            "record",
            summary2.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    let size2 = std::fs::metadata(ledger.path()).unwrap().len();

    assert!(size2 > size1, "ledger must grow, never shrink");
}
