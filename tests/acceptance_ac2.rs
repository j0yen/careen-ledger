//! AC2: `attribute <id> --rebuilt-crates N --wall-secs S` appends a paired
//! record keyed by that id with the rebuild cost; the original record is left
//! byte-for-byte unchanged (append-only proven by a test).
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
        "removed_bytes": 5_000_000u64,
        "removed_entries": 5u64,
        "classes": {}
    });
    write!(f, "{json}").unwrap();
    f
}

#[test]
fn attribute_appends_paired_record_original_unchanged() {
    let ledger = tempfile::Builder::new()
        .suffix(".jsonl")
        .tempfile()
        .unwrap();
    let summary = write_summary("cold-repo");

    // 1. Record a sweep.
    let out = Command::new(binary())
        .args([
            "--ledger",
            ledger.path().to_str().unwrap(),
            "record",
            summary.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let sweep_id = String::from_utf8(out.stdout).unwrap().trim().to_string();

    // Capture the original sweep line byte-for-byte.
    let content_before = std::fs::read_to_string(ledger.path()).unwrap();
    let original_line = content_before.lines().next().unwrap().to_string();

    // 2. Attribute rebuild cost to it.
    let out2 = Command::new(binary())
        .args([
            "--ledger",
            ledger.path().to_str().unwrap(),
            "attribute",
            &sweep_id,
            "--rebuilt-crates",
            "3",
            "--wall-secs",
            "15",
        ])
        .output()
        .unwrap();
    assert!(out2.status.success(), "attribute should exit 0");
    let attr_id = String::from_utf8(out2.stdout).unwrap().trim().to_string();
    assert!(!attr_id.is_empty(), "should print attribution id");

    // 3. The ledger must now have exactly 2 lines.
    let content_after = std::fs::read_to_string(ledger.path()).unwrap();
    let lines: Vec<&str> = content_after
        .lines()
        .filter(|l| !l.trim().is_empty())
        .collect();
    assert_eq!(lines.len(), 2, "should have 2 lines after attribute");

    // 4. The original sweep line must be byte-for-byte unchanged.
    assert_eq!(
        lines[0], original_line,
        "original sweep line must not be modified"
    );

    // 5. The attribution line must reference the sweep id.
    let attr_obj: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
    assert_eq!(attr_obj["kind"], "attribution");
    assert_eq!(attr_obj["sweep_id"].as_str().unwrap(), sweep_id);
    assert!(attr_obj["rebuild_cost_bytes"].as_u64().unwrap() > 0);
}
