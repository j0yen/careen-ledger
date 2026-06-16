# careen-ledger

Was-it-worth-it accounting for Cargo `target/` sweeps.

Part of the [wintermute](https://github.com/j0yen/wintermute) fleet — the feedback loop that makes careen-guard smarter than `cargo clean`.

## Overview

Careening a `target/` directory reclaims disk now but forces the *next* build to recompile what was pruned. A naive `cargo clean` on a repo you're about to rebuild nets negative. `careen-ledger` records each sweep's reclaimed bytes **and** the rebuild cost it provoked, so the fleet learns which repos are worth careening (cold, rarely rebuilt) versus which just thrash the compiler (hot, rebuilt daily).

## Subcommands

### `record <summary.json>`

Append one immutable JSONL line for a careen-sweep event and print the new entry id:

```
$ careen-ledger record /tmp/sweep-summary.json
3f2b1a4e-0c9d-4a8e-b0f1-e3d2c1a09876
```

The summary JSON must contain `repo`, `removed_bytes`, `removed_entries`, and optionally `classes` and `ts`.

### `attribute <id> --rebuilt-crates N --wall-secs S`

Pair a rebuild cost with a prior sweep record. The original sweep line is never modified (append-only):

```
$ careen-ledger attribute 3f2b1a4e-... --rebuilt-crates 42 --wall-secs 120
8d4e9f1c-...
```

Or derive the crate count from a `cargo build --message-format=json` log:

```
$ careen-ledger attribute 3f2b1a4e-... --build-log /tmp/cargo-build.json
```

### `verdict <repo>`

Roll up a repo's history into a `worth_careening` recommendation:

```json
{
  "repo": "my-crate",
  "reclaimed_bytes_total": 9500000000,
  "rebuild_cost_total": 420000000,
  "worth_careening": true,
  "rule": "worth_careening = (reclaimed_bytes_total=9500000000) > 2 * (rebuild_cost_bytes_equivalent=420000000); rebuild cost = rebuilt_crates*10000000 + wall_secs*1000000",
  "status": "ok"
}
```

A repo with no history returns `"status": "insufficient-data"` — not an error.

## Worth-careening rule (deterministic, documented)

```
worth_careening = reclaimed_bytes_total > 2 * rebuild_cost_bytes_equivalent

rebuild_cost_bytes_equivalent = rebuilt_crates * 10_000_000
                              + wall_secs      * 1_000_000
```

Constants are frozen: changing them would change historical verdicts.

## Ledger format

Append-only JSONL at `~/.local/state/careen/ledger.jsonl`. Every line is either a `sweep` record or an `attribution` record. Neither kind is ever overwritten. Corrupt lines (bad JSON) are skipped with a warning on read.

Use `--ledger <path>` to override the default path.

## Acceptance criteria

All 8 acceptance criteria are verified by `cargo test`:

| AC | Level | Verified by |
|---|---|---|
| AC1 | MUST | tests/acceptance_ac1.rs |
| AC2 | MUST | tests/acceptance_ac2.rs |
| AC3 | MUST | tests/acceptance_ac3.rs |
| AC4 | MUST | tests/acceptance_ac4.rs |
| AC5 | MUST | tests/acceptance_ac5.rs |
| AC6 | MUST | tests/acceptance_ac6.rs |
| AC7 | MUST | tests/acceptance_ac7.rs |
| AC8 | MUST | tests/acceptance_ac8.rs |

## Installation

```
cargo install --path .
```

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at your option.
