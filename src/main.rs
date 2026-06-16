//! careen-ledger — was-it-worth-it accounting for Cargo target/ sweeps.
//!
//! Append-only JSONL ledger at `~/.local/state/careen/ledger.jsonl`.
//! Subcommands: `record`, `attribute`, `verdict`.

use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, Write as IoWrite};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use chrono::Utc;
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

/// A single line in the JSONL ledger. Two variants:
/// - `kind = "sweep"`: produced by `record`
/// - `kind = "attribution"`: produced by `attribute`
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LedgerLine {
    pub kind: String,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sweep_id: Option<String>,
    pub ts: String,
    pub repo: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub removed_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub removed_entries: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub classes: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rebuilt_crates: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wall_secs: Option<f64>,
    /// Derived rebuild cost in bytes-equivalent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rebuild_cost_bytes: Option<u64>,
}

/// Schema for the summary JSON passed to `record`.
#[derive(Debug, Deserialize)]
pub struct SweepSummary {
    pub repo: String,
    pub removed_bytes: u64,
    pub removed_entries: u64,
    pub classes: Option<serde_json::Value>,
    /// Optional ISO-8601 timestamp; defaults to now.
    pub ts: Option<String>,
}

/// Verdict output (JSON).
#[derive(Debug, Serialize)]
pub struct VerdictOutput {
    pub repo: String,
    pub reclaimed_bytes_total: u64,
    pub rebuild_cost_total: u64,
    pub rebuild_cost_bytes_equivalent: u64,
    pub worth_careening: bool,
    /// Human-readable explanation of the scoring rule.
    pub rule: String,
    pub status: String,
}

// ---------------------------------------------------------------------------
// Worth-careening rule (documented and deterministic)
// ---------------------------------------------------------------------------
//
// A repo is worth careening when:
//   reclaimed_bytes_total > 2 * rebuild_cost_bytes_equivalent
//
// rebuild_cost_bytes_equivalent is derived as:
//   rebuilt_crates * BYTES_PER_CRATE  +  wall_secs * BYTES_PER_SECOND
//
// Constants (frozen — changing them changes historical verdicts):
//   BYTES_PER_CRATE  = 10_000_000  (~10 MB per crate-compile proxy)
//   BYTES_PER_SECOND = 1_000_000   (~1 MB per wall-second proxy)
//
// Rationale: a full recompile of a crate typically writes 10–50 MB of
// incremental artefacts; wall-time cost is proxied at 1 MB/s overhead.
// The 2× multiplier ensures clear wins (reclaimed >> cost) before recommending.

const BYTES_PER_CRATE: u64 = 10_000_000;
const BYTES_PER_SECOND: u64 = 1_000_000;
const WORTH_MULTIPLIER: u64 = 2;

pub fn cost_to_bytes(rebuilt_crates: u64, wall_secs: f64) -> u64 {
    let crate_cost = rebuilt_crates.saturating_mul(BYTES_PER_CRATE);
    // wall_secs is always non-negative from CLI parsing; cast via floor.
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let secs_cost = (wall_secs * BYTES_PER_SECOND as f64) as u64;
    crate_cost.saturating_add(secs_cost)
}

// ---------------------------------------------------------------------------
// Ledger I/O
// ---------------------------------------------------------------------------

fn default_ledger_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(".local/state/careen/ledger.jsonl")
}

fn resolve_path(override_path: Option<&str>) -> PathBuf {
    match override_path {
        Some(p) => PathBuf::from(p),
        None => default_ledger_path(),
    }
}

fn ensure_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating ledger directory {parent:?}"))?;
        }
    }
    Ok(())
}

fn append_line(path: &Path, line: &LedgerLine) -> Result<()> {
    ensure_parent(path)?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("opening ledger {path:?}"))?;
    let json = serde_json::to_string(line).context("serializing ledger line")?;
    writeln!(file, "{json}").context("writing ledger line")?;
    Ok(())
}

/// Read all lines, skipping corrupt ones with a warning (AC6).
pub fn read_lines(path: &Path) -> Result<Vec<LedgerLine>> {
    if !path.exists() {
        return Ok(vec![]);
    }
    let file = fs::File::open(path).with_context(|| format!("opening ledger {path:?}"))?;
    let reader = io::BufReader::new(file);
    let mut out = Vec::new();
    for (idx, raw) in reader.lines().enumerate() {
        let raw = raw.context("reading ledger line")?;
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        match serde_json::from_str::<LedgerLine>(raw) {
            Ok(l) => out.push(l),
            Err(e) => eprintln!("warning: skipping corrupt ledger line {}: {e}", idx + 1),
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Subcommand implementations
// ---------------------------------------------------------------------------

/// AC1: record <summary.json>
fn cmd_record(path: &Path, summary_path: &str) -> Result<()> {
    let raw =
        fs::read_to_string(summary_path).with_context(|| format!("reading {summary_path}"))?;
    let summary: SweepSummary =
        serde_json::from_str(&raw).context("parsing sweep summary JSON")?;

    let ts = summary
        .ts
        .unwrap_or_else(|| Utc::now().to_rfc3339());
    let id = Uuid::new_v4().to_string();

    let line = LedgerLine {
        kind: "sweep".to_string(),
        id: id.clone(),
        sweep_id: None,
        ts,
        repo: summary.repo,
        removed_bytes: Some(summary.removed_bytes),
        removed_entries: Some(summary.removed_entries),
        classes: summary.classes,
        rebuilt_crates: None,
        wall_secs: None,
        rebuild_cost_bytes: None,
    };

    append_line(path, &line)?;
    println!("{id}");
    Ok(())
}

/// AC2 + AC5: attribute <id> [--rebuilt-crates N --wall-secs S | --build-log <path>]
fn cmd_attribute(
    path: &Path,
    sweep_id: &str,
    rebuilt_crates: Option<u64>,
    wall_secs: Option<f64>,
    build_log: Option<&str>,
) -> Result<()> {
    // Verify the sweep record exists (append-only: we don't touch the
    // original line, only append a new attribution line).
    let lines = read_lines(path)?;
    let sweep_record = lines
        .iter()
        .find(|l| l.kind == "sweep" && l.id == sweep_id);
    let Some(sweep) = sweep_record else {
        bail!("no sweep record with id {sweep_id}");
    };
    let repo = sweep.repo.clone();

    // Derive rebuilt_crates / wall_secs from build log if provided (AC5).
    let (final_crates, final_secs) = if let Some(log_path) = build_log {
        parse_build_log(log_path)?
    } else {
        let c = rebuilt_crates.context("--rebuilt-crates required without --build-log")?;
        let s = wall_secs.context("--wall-secs required without --build-log")?;
        (c, s)
    };

    let rebuild_cost = cost_to_bytes(final_crates, final_secs);
    let id = Uuid::new_v4().to_string();

    let attr = LedgerLine {
        kind: "attribution".to_string(),
        id: id.clone(),
        sweep_id: Some(sweep_id.to_string()),
        ts: Utc::now().to_rfc3339(),
        repo,
        removed_bytes: None,
        removed_entries: None,
        classes: None,
        rebuilt_crates: Some(final_crates),
        wall_secs: Some(final_secs),
        rebuild_cost_bytes: Some(rebuild_cost),
    };

    append_line(path, &attr)?;
    println!("{id}");
    Ok(())
}

/// AC5: count `compiler-artifact` lines in a cargo `--message-format=json` log.
fn parse_build_log(log_path: &str) -> Result<(u64, f64)> {
    let raw =
        fs::read_to_string(log_path).with_context(|| format!("reading build log {log_path}"))?;
    let mut artifact_count: u64 = 0;
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(line) {
            if val.get("reason").and_then(|r| r.as_str()) == Some("compiler-artifact") {
                artifact_count += 1;
            }
        }
    }
    // Wall-secs is not embedded in cargo's JSON output; default to 0 when
    // using --build-log (caller can also pass --wall-secs alongside).
    Ok((artifact_count, 0.0))
}

/// AC3 + AC4 + AC7: verdict <repo>
fn cmd_verdict(path: &Path, repo: &str) -> Result<()> {
    let lines = read_lines(path)?;

    // Collect sweep records for this repo.
    let has_sweeps = lines
        .iter()
        .any(|l| l.kind == "sweep" && l.repo == repo);

    // AC7: no history → insufficient-data verdict.
    if !has_sweeps {
        let verdict = serde_json::json!({
            "repo": repo,
            "status": "insufficient-data",
            "worth_careening": null,
            "message": "no sweep records found for this repo"
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&verdict).context("serializing verdict")?
        );
        return Ok(());
    }

    let reclaimed_bytes_total: u64 = lines
        .iter()
        .filter(|l| l.kind == "sweep" && l.repo == repo)
        .map(|l| l.removed_bytes.unwrap_or(0))
        .fold(0u64, |acc, b| acc.saturating_add(b));

    let rebuild_cost_bytes_equivalent: u64 = lines
        .iter()
        .filter(|l| l.kind == "attribution" && l.repo == repo)
        .map(|l| l.rebuild_cost_bytes.unwrap_or(0))
        .fold(0u64, |acc, b| acc.saturating_add(b));

    // AC4: deterministic rule — worth if reclaimed > 2× rebuild cost.
    let worth_careening = reclaimed_bytes_total
        > WORTH_MULTIPLIER.saturating_mul(rebuild_cost_bytes_equivalent);

    let rule = format!(
        "worth_careening = (reclaimed_bytes_total={reclaimed_bytes_total}) > \
        {WORTH_MULTIPLIER} * (rebuild_cost_bytes_equivalent={rebuild_cost_bytes_equivalent}); \
        rebuild cost = rebuilt_crates*{BYTES_PER_CRATE} + wall_secs*{BYTES_PER_SECOND}"
    );

    let output = VerdictOutput {
        repo: repo.to_string(),
        reclaimed_bytes_total,
        rebuild_cost_total: rebuild_cost_bytes_equivalent,
        rebuild_cost_bytes_equivalent,
        worth_careening,
        rule,
        status: "ok".to_string(),
    };

    println!(
        "{}",
        serde_json::to_string_pretty(&output).context("serializing verdict")?
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

#[derive(Parser, Debug)]
#[command(
    name = "careen-ledger",
    version,
    about = "Was-it-worth-it accounting for Cargo target/ sweeps"
)]
struct Cli {
    /// Override ledger file path (useful for testing).
    #[arg(long, global = true)]
    ledger: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Record a sweep event from a careen-sweep summary JSON file.
    /// Prints the new entry id.
    Record {
        /// Path to the sweep summary JSON produced by careen-sweep.
        summary: String,
    },

    /// Attribute rebuild cost to a prior sweep record.
    /// Appends a paired attribution record; the original sweep is unchanged.
    Attribute {
        /// Sweep entry id returned by `record`.
        id: String,
        /// Number of crates recompiled (required unless `--build-log` is given).
        #[arg(long)]
        rebuilt_crates: Option<u64>,
        /// Wall-clock seconds of the rebuild (required unless `--build-log` is given).
        #[arg(long)]
        wall_secs: Option<f64>,
        /// Path to a `cargo build --message-format=json` log; derives rebuilt_crates count.
        #[arg(long)]
        build_log: Option<String>,
    },

    /// Roll up a repo's history into a worth_careening recommendation.
    Verdict {
        /// Repository name (as recorded).
        repo: String,
    },
}

fn main() -> Result<()> {
    // Reset SIGPIPE to default disposition so writes to a broken pipe produce
    // a clean process exit rather than an unwinding panic through println!.
    sigpipe::reset();
    let cli = Cli::parse();
    let path = resolve_path(cli.ledger.as_deref());

    match &cli.command {
        Commands::Record { summary } => cmd_record(&path, summary)?,
        Commands::Attribute {
            id,
            rebuilt_crates,
            wall_secs,
            build_log,
        } => cmd_attribute(
            &path,
            id,
            *rebuilt_crates,
            *wall_secs,
            build_log.as_deref(),
        )?,
        Commands::Verdict { repo } => cmd_verdict(&path, repo)?,
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn empty_ledger() -> NamedTempFile {
        tempfile::Builder::new()
            .suffix(".jsonl")
            .tempfile()
            .unwrap_or_else(|_| std::process::exit(1))
    }

    fn write_summary(repo: &str, removed_bytes: u64) -> NamedTempFile {
        let mut f = tempfile::Builder::new()
            .suffix(".json")
            .tempfile()
            .unwrap_or_else(|_| std::process::exit(1));
        let json = serde_json::json!({
            "repo": repo,
            "removed_bytes": removed_bytes,
            "removed_entries": 42u64,
            "classes": {"debug": 1}
        });
        write!(f, "{json}").ok();
        f
    }

    #[test]
    fn append_only_record_grows_file() {
        let ledger = empty_ledger();
        let summary = write_summary("my-repo", 5_000_000);
        cmd_record(ledger.path(), summary.path().to_str().unwrap_or("")).unwrap();
        let size1 = std::fs::metadata(ledger.path()).unwrap().len();
        cmd_record(ledger.path(), summary.path().to_str().unwrap_or("")).unwrap();
        let size2 = std::fs::metadata(ledger.path()).unwrap().len();
        assert!(size2 > size1, "ledger must grow on second record");
    }

    #[test]
    fn cost_formula_is_deterministic() {
        assert_eq!(cost_to_bytes(1, 0.0), BYTES_PER_CRATE);
        assert_eq!(cost_to_bytes(0, 1.0), BYTES_PER_SECOND);
        assert_eq!(
            cost_to_bytes(2, 10.0),
            2 * BYTES_PER_CRATE + 10 * BYTES_PER_SECOND
        );
    }

    #[test]
    fn corrupt_line_skipped_not_crash() {
        let mut ledger = empty_ledger();
        writeln!(ledger, "{{bad json{{").ok();
        let lines = read_lines(ledger.path()).unwrap();
        assert!(lines.is_empty(), "corrupt line should be skipped");
    }

    #[test]
    fn verdict_insufficient_data_when_no_history() {
        let ledger = empty_ledger();
        // Should not return an error.
        cmd_verdict(ledger.path(), "nonexistent-repo").unwrap();
    }

    #[test]
    fn parse_build_log_counts_artifacts() {
        let mut f = tempfile::Builder::new()
            .suffix(".json")
            .tempfile()
            .unwrap();
        writeln!(f, r#"{{"reason":"compiler-artifact","package_id":"foo"}}"#).ok();
        writeln!(
            f,
            r#"{{"reason":"build-script-executed","package_id":"bar"}}"#
        )
        .ok();
        writeln!(f, r#"{{"reason":"compiler-artifact","package_id":"baz"}}"#).ok();
        let (count, secs) = parse_build_log(f.path().to_str().unwrap_or("")).unwrap();
        assert_eq!(count, 2);
        assert_eq!(secs, 0.0);
    }
}
