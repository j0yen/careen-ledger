//! AC7: `verdict` on a repo with no history exits cleanly with an
//! `insufficient-data` JSON verdict, not an error.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::process::Command;

fn binary() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_careen-ledger"))
}

#[test]
fn verdict_no_history_insufficient_data() {
    let ledger = tempfile::Builder::new()
        .suffix(".jsonl")
        .tempfile()
        .unwrap();

    let out = Command::new(binary())
        .args([
            "--ledger",
            ledger.path().to_str().unwrap(),
            "verdict",
            "never-swept-repo",
        ])
        .output()
        .unwrap();

    // Must exit cleanly (not an error exit).
    assert!(
        out.status.success(),
        "verdict on unknown repo must exit 0, not error"
    );

    let stdout = String::from_utf8(out.stdout).unwrap();
    let obj: serde_json::Value =
        serde_json::from_str(&stdout).expect("verdict must print valid JSON");

    // Status must be insufficient-data.
    assert_eq!(
        obj["status"].as_str().unwrap(),
        "insufficient-data",
        "status must be 'insufficient-data'"
    );
}

#[test]
fn verdict_no_history_empty_ledger_file() {
    // Ledger file doesn't even exist yet — should also be clean.
    let dir = tempfile::tempdir().unwrap();
    let ledger_path = dir.path().join("nonexistent.jsonl");

    let out = Command::new(binary())
        .args([
            "--ledger",
            ledger_path.to_str().unwrap(),
            "verdict",
            "ghost-repo",
        ])
        .output()
        .unwrap();

    assert!(out.status.success(), "verdict with no ledger file must exit 0");
    let stdout = String::from_utf8(out.stdout).unwrap();
    let obj: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(obj["status"].as_str().unwrap(), "insufficient-data");
}
