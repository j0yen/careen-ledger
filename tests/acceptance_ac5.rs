//! AC5: `attribute` can derive rebuild cost from a cargo `--message-format=json`
//! build log (counting `compiler-artifact` lines) when `--build-log <path>` is given.

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
        "repo": "log-repo",
        "removed_bytes": 8_000_000u64,
        "removed_entries": 8u64,
        "classes": {}
    });
    write!(f, "{json}").unwrap();
    f
}

fn write_build_log(artifact_count: u64) -> tempfile::NamedTempFile {
    let mut f = tempfile::Builder::new()
        .suffix(".json")
        .tempfile()
        .unwrap();
    for i in 0..artifact_count {
        let line = serde_json::json!({
            "reason": "compiler-artifact",
            "package_id": format!("pkg-{i}")
        });
        writeln!(f, "{line}").unwrap();
    }
    // Also add a non-artifact line.
    let other = serde_json::json!({"reason": "build-script-executed", "package_id": "build"});
    writeln!(f, "{other}").unwrap();
    f
}

#[test]
fn attribute_derives_cost_from_build_log() {
    let ledger = tempfile::Builder::new()
        .suffix(".jsonl")
        .tempfile()
        .unwrap();
    let summary = write_summary();

    // Record.
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

    // Build log with 7 compiler-artifact lines.
    let build_log = write_build_log(7);

    // Attribute using --build-log.
    let out2 = Command::new(binary())
        .args([
            "--ledger",
            ledger.path().to_str().unwrap(),
            "attribute",
            &sweep_id,
            "--build-log",
            build_log.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(out2.status.success(), "attribute --build-log should succeed");

    // Verify attribution has rebuild cost > 0 (7 crates * 10 MB each = 70 MB).
    let content = std::fs::read_to_string(ledger.path()).unwrap();
    let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(lines.len(), 2);

    let attr: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
    assert_eq!(attr["kind"], "attribution");
    let cost = attr["rebuild_cost_bytes"].as_u64().unwrap();
    // 7 crates * 10_000_000 = 70_000_000
    assert_eq!(cost, 70_000_000, "expected 7 crates * 10MB = 70MB");
    assert_eq!(attr["rebuilt_crates"].as_u64().unwrap(), 7);
}
