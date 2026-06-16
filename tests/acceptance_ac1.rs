//! AC1: `careen-ledger record <summary.json>` appends one immutable JSONL line
//! carrying repo, ts, removed_bytes, and classes, and prints the new entry id.

use std::io::Write as _;
use std::process::Command;

fn binary() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_careen-ledger"))
}

fn write_summary(repo: &str, removed_bytes: u64) -> tempfile::NamedTempFile {
    let mut f = tempfile::Builder::new()
        .suffix(".json")
        .tempfile()
        .unwrap();
    let json = serde_json::json!({
        "repo": repo,
        "removed_bytes": removed_bytes,
        "removed_entries": 10u64,
        "classes": {"debug": 3}
    });
    write!(f, "{json}").unwrap();
    f
}

#[test]
fn record_appends_one_line_with_required_fields() {
    let ledger = tempfile::Builder::new()
        .suffix(".jsonl")
        .tempfile()
        .unwrap();
    let summary = write_summary("my-repo", 9_000_000);

    let out = Command::new(binary())
        .args([
            "--ledger",
            ledger.path().to_str().unwrap(),
            "record",
            summary.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(out.status.success(), "record should exit 0");

    // Printed id should be non-empty.
    let printed_id = String::from_utf8(out.stdout).unwrap().trim().to_string();
    assert!(!printed_id.is_empty(), "should print entry id");

    // Ledger should contain exactly one line.
    let content = std::fs::read_to_string(ledger.path()).unwrap();
    let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(lines.len(), 1, "exactly one JSONL line should be appended");

    // That line should be valid JSON with required fields.
    let obj: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(obj["repo"], "my-repo");
    assert_eq!(obj["removed_bytes"], 9_000_000u64);
    assert!(obj["ts"].is_string(), "ts field required");
    assert!(obj["classes"].is_object(), "classes field required");
    assert_eq!(obj["id"].as_str().unwrap(), printed_id);
}
