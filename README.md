# careen-ledger

Append-only accounting that tells you whether sweeping a Cargo `target/` was worth it.

## Why it exists

Reclaiming a `target/` directory buys disk now but bills the next build, which has to recompile everything that got pruned. On a cold repo that is free money. On a repo you rebuild every day it is a loss — you paid in compiler time for space you immediately filled back in. The only way to know which case you are in is to record both sides of the ledger: the bytes a sweep reclaimed, and the rebuild cost that sweep provoked. `careen-ledger` keeps that record and turns a repo's history into a verdict.

## Install

```sh
cargo install --path .
```

## How it works

The ledger is an append-only JSONL file. Every line is either a `sweep` record (what was reclaimed) or an `attribution` record (the rebuild it cost). Lines are never rewritten — `attribute` adds a new line paired to a prior sweep by id rather than editing the original. Default path is `~/.local/state/careen/ledger.jsonl`; override with `--ledger <path>`.

A verdict rolls a repo's history into one boolean against a frozen rule:

```
worth_careening = reclaimed_bytes_total > 2 * rebuild_cost_bytes_equivalent

rebuild_cost_bytes_equivalent = rebuilt_crates * 10_000_000   (~10 MB per crate-compile)
                              + wall_secs      *  1_000_000   (~1 MB per wall-second)
```

The constants are deliberately frozen: changing them would silently rewrite the verdict on every past sweep. The 2× margin means a repo only earns "worth careening" on a clear win, not a coin flip.

## Quickstart

Record a sweep from a [careen-sweep](https://github.com/j0yen/careen-sweep) summary; the command prints the new entry id.

```sh
$ careen-ledger record /tmp/sweep-summary.json
3f2b1a4e-0c9d-4a8e-b0f1-e3d2c1a09876
```

The summary JSON needs `repo`, `removed_bytes`, `removed_entries`, and optionally `classes` and `ts`.

Pair the rebuild cost to that sweep — directly, or derived by counting `compiler-artifact` lines in a `cargo build --message-format=json` log:

```sh
$ careen-ledger attribute 3f2b1a4e-... --rebuilt-crates 42 --wall-secs 120
$ careen-ledger attribute 3f2b1a4e-... --build-log /tmp/cargo-build.json
```

Ask for the verdict:

```sh
$ careen-ledger verdict my-crate
{
  "repo": "my-crate",
  "reclaimed_bytes_total": 9500000000,
  "rebuild_cost_total": 420000000,
  "worth_careening": true,
  "rule": "worth_careening = (reclaimed_bytes_total=9500000000) > 2 * (rebuild_cost_bytes_equivalent=420000000); ...",
  "status": "ok"
}
```

A repo with no history returns `"status": "insufficient-data"` rather than an error — absence of evidence is not a verdict.

Corrupt lines (bad JSON) are skipped with a warning on read, so a partial write can't take the whole ledger down.

## Where it fits

The careen family reclaims bytes inside Cargo `target/` dirs in three steps:

- [careen-survey](https://github.com/j0yen/careen-survey) — read-only classifier of what is reclaimable.
- [careen-sweep](https://github.com/j0yen/careen-sweep) — lock-aware reclaimer that acts on the survey.
- **careen-ledger** — records each sweep and its rebuild cost, and scores whether it paid off.

careen-survey and careen-sweep answer "what can I reclaim, and reclaim it"; careen-ledger answers "and should I have." Part of the [wintermute](https://github.com/j0yen/wintermute) fleet.

## Tests

The eight acceptance criteria are verified by `cargo test` (`tests/acceptance_ac1.rs` … `ac8.rs`), covering append-only behavior, the deterministic cost formula, build-log parsing, the insufficient-data path, and corrupt-line tolerance.

## License

MIT or Apache-2.0, at your option.
