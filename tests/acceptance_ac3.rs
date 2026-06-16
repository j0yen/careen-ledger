//! AC3: `verdict <repo>` folds all paired records for the repo into JSON with
//! `reclaimed_bytes_total`, `rebuild_cost_total`, and a boolean `worth_careening`.

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
        "removed_entries": 20u64,
        "classes": {"fingerprint": 10}
    });
    write!(f, "{json}").unwrap();
    f
}

#[test]
fn verdict_produces_required_fields() {
    let ledger = tempfile::Builder::new()
        .suffix(".jsonl")
        .tempfile()
        .unwrap();

    let summary = write_summary("analyze-repo", 12_000_000);

    // Record + attribute once.
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

    Command::new(binary())
        .args([
            "--ledger",
            ledger.path().to_str().unwrap(),
            "attribute",
            &sweep_id,
            "--rebuilt-crates",
            "1",
            "--wall-secs",
            "5",
        ])
        .output()
        .unwrap();

    // Call verdict.
    let out3 = Command::new(binary())
        .args([
            "--ledger",
            ledger.path().to_str().unwrap(),
            "verdict",
            "analyze-repo",
        ])
        .output()
        .unwrap();
    assert!(out3.status.success(), "verdict should exit 0");

    let stdout = String::from_utf8(out3.stdout).unwrap();
    let obj: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    // Required fields.
    assert!(
        obj["reclaimed_bytes_total"].as_u64().is_some(),
        "reclaimed_bytes_total required"
    );
    assert!(
        obj["rebuild_cost_total"].as_u64().is_some(),
        "rebuild_cost_total required"
    );
    assert!(
        obj["worth_careening"].is_boolean(),
        "worth_careening must be boolean"
    );
    assert_eq!(obj["reclaimed_bytes_total"].as_u64().unwrap(), 12_000_000);
}
