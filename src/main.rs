#[cfg(unix)]
fn reset_sigpipe() {
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

#[cfg(not(unix))]
fn reset_sigpipe() {}

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Utc;
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
struct LedgerEntry {
    ts: String,
    event: String,
    repo: String,
    target_dir: Option<String>,
    bytes_reclaimed: Option<u64>,
    rebuild_cost_bytes: Option<u64>,
    rebuild_wall_secs: Option<u64>,
    net_bytes: Option<i64>,
}

#[derive(Parser, Debug)]
#[command(name = "careen-ledger", version, about = "Was-it-worth-it accounting for Cargo target/ sweeps")]
struct Cli {
    /// Override ledger file path
    #[arg(long, global = true)]
    ledger: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Record a sweep event
    RecordSweep {
        #[arg(long)]
        repo: String,
        #[arg(long)]
        target: String,
        #[arg(long)]
        bytes: u64,
    },
    /// Record a rebuild event and compute net for last sweep
    RecordRebuild {
        #[arg(long)]
        repo: String,
        #[arg(long = "wall-secs")]
        wall_secs: u64,
        #[arg(long = "bytes-written")]
        bytes_written: u64,
    },
    /// Show aggregated report
    Report {
        #[arg(long)]
        repo: Option<String>,
        #[arg(long)]
        since: Option<u64>,
    },
    /// Show repos sorted by rebuild frequency ascending
    HotRepos {
        #[arg(long, default_value = "10")]
        top: usize,
    },
    /// JSON-RPC 2.0 server over stdio
    Serve,
}

fn ledger_path(override_path: Option<&str>) -> PathBuf {
    if let Some(p) = override_path {
        return PathBuf::from(p);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(".local/share/careen/ledger.jsonl")
}

fn append_entry(path: &Path, entry: &LedgerEntry) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating ledger dir {:?}", parent))?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("opening ledger {:?}", path))?;
    let line = serde_json::to_string(entry).context("serializing entry")?;
    writeln!(file, "{}", line).context("writing entry")?;
    Ok(())
}

fn read_entries(path: &Path) -> Result<Vec<LedgerEntry>> {
    if !path.exists() {
        return Ok(vec![]);
    }
    let file = fs::File::open(path).with_context(|| format!("opening ledger {:?}", path))?;
    let reader = io::BufReader::new(file);
    let mut entries = Vec::new();
    for (i, line) in reader.lines().enumerate() {
        let line = line.context("reading line")?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<LedgerEntry>(line) {
            Ok(e) => entries.push(e),
            Err(err) => eprintln!("warning: skipping bad ledger line {}: {}", i + 1, err),
        }
    }
    Ok(entries)
}

fn cmd_record_sweep(path: &Path, repo: &str, target: &str, bytes: u64) -> Result<()> {
    let entry = LedgerEntry {
        ts: Utc::now().to_rfc3339(),
        event: "sweep".to_string(),
        repo: repo.to_string(),
        target_dir: Some(target.to_string()),
        bytes_reclaimed: Some(bytes),
        rebuild_cost_bytes: None,
        rebuild_wall_secs: None,
        net_bytes: None,
    };
    append_entry(path, &entry)?;
    println!("Recorded sweep: repo={} bytes_reclaimed={}", repo, bytes);
    Ok(())
}

fn cmd_record_rebuild(path: &Path, repo: &str, wall_secs: u64, bytes_written: u64) -> Result<()> {
    let entries = read_entries(path)?;
    // Find most recent sweep for this repo
    let last_sweep = entries
        .iter()
        .rev()
        .find(|e| e.event == "sweep" && e.repo == repo);
    let net_bytes = last_sweep.and_then(|s| {
        s.bytes_reclaimed
            .map(|reclaimed| reclaimed as i64 - bytes_written as i64)
    });
    let entry = LedgerEntry {
        ts: Utc::now().to_rfc3339(),
        event: "rebuild".to_string(),
        repo: repo.to_string(),
        target_dir: None,
        bytes_reclaimed: None,
        rebuild_cost_bytes: Some(bytes_written),
        rebuild_wall_secs: Some(wall_secs),
        net_bytes,
    };
    append_entry(path, &entry)?;
    match net_bytes {
        Some(n) => println!(
            "Recorded rebuild: repo={} wall_secs={} bytes_written={} net_bytes={}",
            repo, wall_secs, bytes_written, n
        ),
        None => println!(
            "Recorded rebuild: repo={} wall_secs={} bytes_written={} (no prior sweep found)",
            repo, wall_secs, bytes_written
        ),
    }
    Ok(())
}

struct RepoStats {
    sweeps: u64,
    total_reclaimed: u64,
    total_rebuild_cost: u64,
    net_bytes: i64,
    rebuild_count: u64,
}

impl RepoStats {
    fn new() -> Self {
        RepoStats {
            sweeps: 0,
            total_reclaimed: 0,
            total_rebuild_cost: 0,
            net_bytes: 0,
            rebuild_count: 0,
        }
    }
}

fn aggregate_entries(entries: &[LedgerEntry]) -> HashMap<String, RepoStats> {
    let mut map: HashMap<String, RepoStats> = HashMap::new();
    for entry in entries {
        let stats = map.entry(entry.repo.clone()).or_insert_with(RepoStats::new);
        if entry.event == "sweep" {
            stats.sweeps += 1;
            stats.total_reclaimed += entry.bytes_reclaimed.unwrap_or(0);
        } else if entry.event == "rebuild" {
            stats.rebuild_count += 1;
            let cost = entry.rebuild_cost_bytes.unwrap_or(0);
            stats.total_rebuild_cost += cost;
            if let Some(net) = entry.net_bytes {
                stats.net_bytes += net;
            }
        }
    }
    map
}

fn filter_entries_since(entries: Vec<LedgerEntry>, since_days: Option<u64>) -> Vec<LedgerEntry> {
    let Some(days) = since_days else {
        return entries;
    };
    let cutoff = Utc::now() - chrono::Duration::days(days as i64);
    let cutoff_str = cutoff.to_rfc3339();
    entries
        .into_iter()
        .filter(|e| e.ts >= cutoff_str)
        .collect()
}

fn cmd_report(path: &Path, repo_filter: Option<&str>, since: Option<u64>) -> Result<()> {
    let entries = read_entries(path)?;
    let entries = filter_entries_since(entries, since);
    let entries: Vec<_> = match repo_filter {
        Some(r) => entries.into_iter().filter(|e| e.repo == r).collect(),
        None => entries,
    };
    let map = aggregate_entries(&entries);
    if map.is_empty() {
        println!("No entries found.");
        return Ok(());
    }
    let mut repos: Vec<_> = map.keys().cloned().collect();
    repos.sort();
    println!(
        "{:<30} {:>8} {:>18} {:>18} {:>14} verdict",
        "repo", "sweeps", "total_reclaimed", "total_rebuild_cost", "net_bytes"
    );
    println!("{}", "-".repeat(100));
    for repo in &repos {
        let stats = &map[repo];
        let verdict = if stats.net_bytes > 0 {
            "worth-it"
        } else if stats.rebuild_count == 0 {
            "no-rebuilds"
        } else {
            "not-worth-it"
        };
        println!(
            "{:<30} {:>8} {:>18} {:>18} {:>14} {}",
            repo,
            stats.sweeps,
            stats.total_reclaimed,
            stats.total_rebuild_cost,
            stats.net_bytes,
            verdict
        );
    }
    Ok(())
}

fn cmd_hot_repos(path: &Path, top: usize) -> Result<()> {
    let entries = read_entries(path)?;
    let map = aggregate_entries(&entries);
    if map.is_empty() {
        println!("No entries found.");
        return Ok(());
    }
    let mut repos: Vec<_> = map.iter().collect();
    // Sort ascending by rebuild_count (fewest rebuilds first = best careen candidates)
    repos.sort_by_key(|(_, stats)| stats.rebuild_count);
    let top_repos: Vec<_> = repos.into_iter().take(top).collect();
    println!("{:<30} {:>15} {:>8}", "repo", "rebuild_count", "sweeps");
    println!("{}", "-".repeat(57));
    for (repo, stats) in &top_repos {
        println!(
            "{:<30} {:>15} {:>8}",
            repo, stats.rebuild_count, stats.sweeps
        );
    }
    Ok(())
}

fn rpc_error(id: &serde_json::Value, code: i64, message: &str) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message
        }
    })
}

fn rpc_ok(id: &serde_json::Value, result: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    })
}

fn cmd_serve(path: &Path) -> Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    for line in stdin.lock().lines() {
        let line = line.context("reading stdin")?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let req: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => {
                let resp = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": {"code": -32700, "message": "Parse error"}
                });
                let mut out = stdout.lock();
                writeln!(out, "{}", resp).context("writing response")?;
                continue;
            }
        };
        let id = req.get("id").cloned().unwrap_or(serde_json::Value::Null);
        let method = req
            .get("method")
            .and_then(|m| m.as_str())
            .unwrap_or("");
        let resp = match method {
            "initialize" => rpc_ok(
                &id,
                serde_json::json!({
                    "name": "careen-ledger",
                    "version": env!("CARGO_PKG_VERSION")
                }),
            ),
            "shutdown" => {
                let resp = rpc_ok(&id, serde_json::Value::Null);
                let mut out = stdout.lock();
                writeln!(out, "{}", resp).context("writing response")?;
                return Ok(());
            }
            "careen/report" => {
                let params = req.get("params");
                let repo_filter = params
                    .and_then(|p| p.get("repo"))
                    .and_then(|r| r.as_str());
                let since = params
                    .and_then(|p| p.get("since"))
                    .and_then(|s| s.as_u64());
                let entries = read_entries(path).unwrap_or_default();
                let entries = filter_entries_since(entries, since);
                let entries: Vec<_> = match repo_filter {
                    Some(r) => entries.into_iter().filter(|e| e.repo == r).collect(),
                    None => entries,
                };
                let map = aggregate_entries(&entries);
                let mut result = Vec::new();
                let mut repos: Vec<_> = map.keys().cloned().collect();
                repos.sort();
                for repo in &repos {
                    let stats = &map[repo];
                    let verdict = if stats.net_bytes > 0 {
                        "worth-it"
                    } else if stats.rebuild_count == 0 {
                        "no-rebuilds"
                    } else {
                        "not-worth-it"
                    };
                    result.push(serde_json::json!({
                        "repo": repo,
                        "sweeps": stats.sweeps,
                        "total_reclaimed": stats.total_reclaimed,
                        "total_rebuild_cost": stats.total_rebuild_cost,
                        "net_bytes": stats.net_bytes,
                        "verdict": verdict
                    }));
                }
                rpc_ok(&id, serde_json::Value::Array(result))
            }
            "careen/hot-repos" => {
                let params = req.get("params");
                let top = params
                    .and_then(|p| p.get("top"))
                    .and_then(|t| t.as_u64())
                    .unwrap_or(10) as usize;
                let entries = read_entries(path).unwrap_or_default();
                let map = aggregate_entries(&entries);
                let mut repos: Vec<_> = map.iter().collect();
                repos.sort_by_key(|(_, stats)| stats.rebuild_count);
                let result: Vec<_> = repos
                    .into_iter()
                    .take(top)
                    .map(|(repo, stats)| {
                        serde_json::json!({
                            "repo": repo,
                            "rebuild_count": stats.rebuild_count,
                            "sweeps": stats.sweeps
                        })
                    })
                    .collect();
                rpc_ok(&id, serde_json::Value::Array(result))
            }
            _ => rpc_error(&id, -32601, "Method not found"),
        };
        let mut out = stdout.lock();
        writeln!(out, "{}", resp).context("writing response")?;
    }
    Ok(())
}

fn main() -> Result<()> {
    reset_sigpipe();
    let cli = Cli::parse();
    let path = ledger_path(cli.ledger.as_deref());
    match &cli.command {
        Commands::RecordSweep { repo, target, bytes } => {
            cmd_record_sweep(&path, repo, target, *bytes)?;
        }
        Commands::RecordRebuild {
            repo,
            wall_secs,
            bytes_written,
        } => {
            cmd_record_rebuild(&path, repo, *wall_secs, *bytes_written)?;
        }
        Commands::Report { repo, since } => {
            cmd_report(&path, repo.as_deref(), *since)?;
        }
        Commands::HotRepos { top } => {
            cmd_hot_repos(&path, *top)?;
        }
        Commands::Serve => {
            cmd_serve(&path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn tmp_ledger() -> NamedTempFile {
        tempfile::Builder::new()
            .suffix(".jsonl")
            .tempfile()
            .expect("tempfile")
    }

    #[test]
    fn test_sweep_rebuild_net_bytes() {
        let f = tmp_ledger();
        let path = f.path();
        cmd_record_sweep(path, "my-repo", "/target", 10_000_000).expect("sweep");
        cmd_record_rebuild(path, "my-repo", 120, 3_000_000).expect("rebuild");
        let entries = read_entries(path).expect("read");
        let rebuild = entries.iter().find(|e| e.event == "rebuild").expect("rebuild entry");
        assert_eq!(rebuild.net_bytes, Some(10_000_000 - 3_000_000));
    }

    #[test]
    fn test_report_aggregation() {
        let f = tmp_ledger();
        let path = f.path();
        cmd_record_sweep(path, "repo-a", "/a/target", 5_000_000).expect("sweep1");
        cmd_record_sweep(path, "repo-a", "/a/target", 3_000_000).expect("sweep2");
        cmd_record_rebuild(path, "repo-a", 60, 1_000_000).expect("rebuild");
        let entries = read_entries(path).expect("read");
        let map = aggregate_entries(&entries);
        let stats = map.get("repo-a").expect("repo-a");
        assert_eq!(stats.sweeps, 2);
        assert_eq!(stats.total_reclaimed, 8_000_000);
        assert_eq!(stats.total_rebuild_cost, 1_000_000);
        assert!(stats.net_bytes > 0);
    }

    #[test]
    fn test_hot_repos_order() {
        let f = tmp_ledger();
        let path = f.path();
        // repo-frequent: 3 rebuilds
        for _ in 0..3 {
            cmd_record_sweep(path, "repo-frequent", "/t", 1_000).expect("sweep");
            cmd_record_rebuild(path, "repo-frequent", 10, 500).expect("rebuild");
        }
        // repo-rare: 1 rebuild
        cmd_record_sweep(path, "repo-rare", "/t", 1_000).expect("sweep");
        cmd_record_rebuild(path, "repo-rare", 10, 500).expect("rebuild");
        let entries = read_entries(path).expect("read");
        let map = aggregate_entries(&entries);
        let mut repos: Vec<_> = map.iter().collect();
        repos.sort_by_key(|(_, s)| s.rebuild_count);
        // repo-rare should come first (fewest rebuilds)
        assert_eq!(repos[0].0, "repo-rare");
    }

    #[test]
    fn test_ledger_append_only() {
        let f = tmp_ledger();
        let path = f.path();
        cmd_record_sweep(path, "repo-x", "/x", 999_000).expect("sweep");
        let size1 = std::fs::metadata(path).expect("meta1").len();
        cmd_record_rebuild(path, "repo-x", 30, 100_000).expect("rebuild");
        let size2 = std::fs::metadata(path).expect("meta2").len();
        assert!(size2 > size1, "ledger should grow, not shrink");
    }
}
