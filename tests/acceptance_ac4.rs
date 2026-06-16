//! AC4: The worth_careening rule is deterministic and documented.
//! Two fixtures (cold-win and hot-thrash) classify oppositely.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

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
        "removed_entries": 1u64,
        "classes": {}
    });
    write!(f, "{json}").unwrap();
    f
}

fn record_and_attribute(
    ledger_path: &std::path::Path,
    repo: &str,
    removed_bytes: u64,
    rebuilt_crates: u64,
    wall_secs: f64,
) {
    let summary = write_summary(repo, removed_bytes);
    let out = Command::new(binary())
        .args([
            "--ledger",
            ledger_path.to_str().unwrap(),
            "record",
            summary.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    let sweep_id = String::from_utf8(out.stdout).unwrap().trim().to_string();

    Command::new(binary())
        .args([
            "--ledger",
            ledger_path.to_str().unwrap(),
            "attribute",
            &sweep_id,
            "--rebuilt-crates",
            &rebuilt_crates.to_string(),
            "--wall-secs",
            &wall_secs.to_string(),
        ])
        .output()
        .unwrap();
}

fn get_worth_careening(ledger_path: &std::path::Path, repo: &str) -> bool {
    let out = Command::new(binary())
        .args([
            "--ledger",
            ledger_path.to_str().unwrap(),
            "verdict",
            repo,
        ])
        .output()
        .unwrap();
    let obj: serde_json::Value =
        serde_json::from_str(&String::from_utf8(out.stdout).unwrap()).unwrap();
    obj["worth_careening"].as_bool().unwrap()
}

#[test]
fn cold_win_repo_worth_careening() {
    // cold-win: reclaims 500 MB, rebuild cost is only 1 crate (10 MB equivalent)
    // → reclaimed (500M) > 2 * cost (10M) → worth_careening = true
    let ledger = tempfile::Builder::new()
        .suffix(".jsonl")
        .tempfile()
        .unwrap();
    record_and_attribute(
        ledger.path(),
        "cold-repo",
        500_000_000, // 500 MB reclaimed
        1,           // 1 crate rebuilt
        0.0,         // 0 wall-secs
    );
    let result = get_worth_careening(ledger.path(), "cold-repo");
    assert!(result, "cold-win repo should be worth careening");
}

#[test]
fn hot_thrash_repo_not_worth_careening() {
    // hot-thrash: reclaims only 5 MB, rebuild cost is 50 crates (500 MB equivalent)
    // → reclaimed (5M) < 2 * cost (500M) → worth_careening = false
    let ledger = tempfile::Builder::new()
        .suffix(".jsonl")
        .tempfile()
        .unwrap();
    record_and_attribute(
        ledger.path(),
        "hot-repo",
        5_000_000,   // 5 MB reclaimed
        50,          // 50 crates rebuilt = 500 MB cost equivalent
        0.0,
    );
    let result = get_worth_careening(ledger.path(), "hot-repo");
    assert!(!result, "hot-thrash repo should NOT be worth careening");
}

#[test]
fn two_fixtures_classify_oppositely() {
    let ledger = tempfile::Builder::new()
        .suffix(".jsonl")
        .tempfile()
        .unwrap();
    // cold-win in same ledger.
    record_and_attribute(ledger.path(), "cold-repo2", 500_000_000, 1, 0.0);
    // hot-thrash in same ledger.
    record_and_attribute(ledger.path(), "hot-repo2", 5_000_000, 50, 0.0);

    let cold_verdict = get_worth_careening(ledger.path(), "cold-repo2");
    let hot_verdict = get_worth_careening(ledger.path(), "hot-repo2");

    assert_ne!(
        cold_verdict, hot_verdict,
        "cold and hot repos must classify oppositely"
    );
}
