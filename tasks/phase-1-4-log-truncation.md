# Phase 1.4 — Log Storage Truncation and Backpressure

**Phase:** 1 (Safety Hardening)
**Priority:** 🔴 High
**Estimated effort:** 2-3 days

## Problem

`crates/db/src/models/execution_process_logs.rs:51-68` appends unbounded JSONL log rows to SQLite with:
- no per-execution byte cap
- no truncation marker
- no batching or backpressure on bursty writes

A runaway agent (print loop, unbounded build output, malformed binary written to stdout) will balloon the SQLite database, slow every subsequent query, and freeze the UI replaying the stream.

The full architectural fix (logs as append-only files with offset checkpoints) is deferred to Phase 3.5. This task lands the urgent caps and batching while keeping logs in SQLite.

## Deliverable

1. **Config:**
   - New current config field (`crates/services/src/services/config/versions/v8.rs` today; likely becomes v9): `max_log_bytes_per_execution: u64` (default 50 MB; configurable per install).
   - Surfaced in diagnostics (Phase 3.1) but no settings UI required in this task — env var or config file is fine.
   - Export through `crates/server/src/bin/generate_types.rs`; do not edit `shared/types.ts` by hand.

2. **Per-execution byte tracking:**
   - New column on `execution_processes`: `log_bytes_written: i64` (migration).
   - New column: `log_truncated: bool` default `false`.
   - Increment `log_bytes_written` atomically on each log batch insert.

3. **Truncation enforcement:**
   - Before inserting a log batch, check `log_bytes_written + batch_size` against the cap.
   - If the cap would be exceeded:
     - Insert a final truncation marker. `LogMsg` currently has no `System` variant, so either add one deliberately and update all renderers, or use a clearly-prefixed `LogMsg::Stderr` marker for the minimal version.
     - Set `log_truncated = true`.
     - Drop subsequent log writes for this execution silently (with a warn-level trace).
   - Enforce this inside a SQL transaction so concurrent log writers cannot both pass the cap check.

4. **Batching / backpressure:**
   - Start at `spawn_stream_raw_logs_to_db` in `crates/services/src/services/container.rs`, which currently serializes and inserts each stdout/stderr message one line at a time.
   - Aggregate log writes in memory per execution process: flush when batch reaches 100 lines or 100ms (whichever first).
   - Update direct error-path writes in `container.rs` to call the same capped append API rather than bypassing truncation.
   - On process exit, flush any remaining buffered lines before marking the execution complete.
   - If the in-memory buffer grows beyond a hard limit (e.g., 10k lines waiting to flush), apply backpressure: block the reader of the child process pipe. Do not silently drop.

5. **UI surface (minimal):**
   - When `log_truncated = true`, the log viewer shows a clear banner: "Logs truncated at {N} bytes."
   - Frontend type generation includes the new field via `pnpm run generate-types`.

## Tests

Unit tests in `crates/db`:
- Insert log batch under cap → `log_bytes_written` updated, `log_truncated` false.
- Insert log batch that crosses cap → truncation marker inserted, `log_truncated` true, subsequent writes dropped.
- Insert log batch after truncation → no-op.

Integration tests in `crates/services` or `crates/local-deployment`:
- Spawn a child process that emits more than a small test cap (e.g. 1 MB against a 64 KB cap) → SQLite size stays bounded near the cap, `log_truncated` true, app remains responsive.
- Spawn a child process that emits bursty output (10k lines in 50ms then idle) → all lines persisted in batched inserts, no loss, no UI freeze.
- Keep the 1 GB scenario as a manual/ignored stress test, not a default CI test.

## Acceptance Criteria

- A runaway agent producing 1 GB of stdout does not balloon the SQLite database.
- The execution row clearly indicates truncation when it occurs.
- The UI shows a truncation banner, not just a sudden silence.
- Bursty output is batched, not inserted line-by-line.
- All listed tests pass.

## Dependencies

None. Phase 3.5 (logs-as-files) builds on this but is not in scope here.

## Notes

- Do not try to make the cap retroactive for existing rows. Migration only adds columns with defaults.
- The 50 MB default is a starting point. Document it as configurable; tune after observing real workloads.
- Resist the temptation to start the file-based storage refactor in this task. Truncation is the urgent safety fix; the architecture change is its own multi-week project (Phase 3.5).
