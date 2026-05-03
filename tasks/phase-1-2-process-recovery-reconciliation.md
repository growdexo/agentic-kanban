# Phase 1.2 — Process Recovery Reconciliation

**Phase:** 1 (Safety Hardening)
**Priority:** 🔴 High
**Estimated effort:** 2-3 days

## Problem

`crates/services/src/services/container.rs:257-307` (`cleanup_orphan_executions`) marks all execution processes in `running` state as `failed` on app startup without validating PIDs or attempting reattachment. This loses the distinction between "process actually died while we were down" and "process is still alive and we just lost track of it," and it produces misleading status for users reviewing past attempts.

The PRD requires reconciliation: detect alive processes, reattach where possible, mark genuinely-gone processes with an accurate terminal state.

## Deliverable

1. **Persist process identity first:**
   - Current `execution_processes` rows do **not** store OS PID or command text; `command` was dropped in migration `20250727124142_remove_command_from_execution_process.sql`, and live children are only tracked in `LocalDeployment.child_store`.
   - Add columns such as `os_pid`, `process_group_id` (if available), and `command_snapshot` / `argv_snapshot` to `execution_processes`.
   - Populate those fields immediately after `executor_action.spawn(...)` succeeds in `crates/local-deployment/src/container.rs`, before inserting the child into `child_store`.

2. **PID validation helper** in `crates/utils/src/process.rs` (or new module):
   - `is_pid_alive(pid: u32) -> bool` — uses `kill(pid, 0)` on Unix.
   - `pid_command_matches(pid: u32, expected_command: &str) -> bool` — best-effort match via `/proc/{pid}/cmdline` on Linux, `ps` on macOS. Returns `false` (not error) on platforms where unavailable.

3. **Reconciliation pass** in `cleanup_orphan_executions`:
   - For each `running` execution process, read persisted PID and command snapshot.
   - If PID is alive AND command matches: keep status as `running`, log a recovery event, and attach a replacement lightweight PID monitor. Do not claim stdout/stderr re-streaming is recovered after restart.
   - If PID is dead OR command does not match: transition to `killed` with new failure code `recovery_orphaned`.
   - If PID is null/missing in DB: transition to `killed` with failure code `recovery_orphaned`.
   - Because stdout/stderr pipes and the in-memory exit monitor are lost after app restart, an alive recovered process must also get a replacement lightweight PID monitor. Poll for process exit and then mark the row terminal with an explicit `recovery_exit_unknown` / `recovery_orphaned` reason. Do not promise exact exit code recovery.

4. **Recovery summary record:**
   - New table `startup_recovery_summary` (migration) or new singleton row capturing: timestamp, total `running` found, reattached count, orphaned count, list of affected execution_process ids.
   - Persist on each startup pass.
   - Surface in the diagnostics page (Phase 3.1) — no UI work required in this task; just persist.

5. **Failure/reason persistence:**
   - The current status enum has `Running`, `Completed`, `Failed`, and `Killed`, but no failure-code column. Add a minimal reason-code field now or explicitly depend on Phase 3.4.
   - Do not overload `exit_code` or free-form logs as the only durable recovery reason.

6. **Logging:**
   - Each reconciliation decision logged at INFO with execution_process id, pid, decision, reason.

## Tests

Unit tests:
- `is_pid_alive` returns `true` for current process pid.
- `is_pid_alive` returns `false` for a PID known to be unused (e.g., `u32::MAX`).
- `pid_command_matches` matches the current process command against itself.

Integration tests with fixtures in `crates/services`:
- Stale execution process row with dead PID → transitions to `killed`/`recovery_orphaned`.
- Stale execution process row with alive matching PID (spawn a `sleep 60` child for the test) → remains `running`.
- Stale execution process row with alive PID but different command → transitions to `killed`/`recovery_orphaned`.
- Stale execution process row with null PID → transitions to `killed`/`recovery_orphaned`.
- Recovery summary record is written and includes correct counts.

## Acceptance Criteria

- Restarting the app while an agent is genuinely running does not flip its status to `failed`.
- Restarting the app after an agent has crashed produces `killed` (not `failed`) with reason `recovery_orphaned`.
- Diagnostics shows the last startup recovery summary.
- Manual test: run an attempt, send `SIGSTOP` to the app process, kill the agent externally, send `SIGCONT` (or restart) → execution row shows `killed`/`recovery_orphaned` after recovery.
- Manual test: restart while a child process is still alive → row remains visible as recovered/running, and eventually transitions out of `running` when the replacement PID monitor observes exit.

## Dependencies

None functional. Phase 3.1 (diagnostics page) consumes the summary record but is not blocking.

## Notes

- Do not attempt to re-stream stdout/stderr of reattached processes via `/proc/{pid}/fd/*` — too platform-specific and fragile. Document that logs produced while the app was down may be missing.
- Reattachment is best-effort. Correctness requirement is only that the *status* is no longer falsely marked `failed`; recovering log streams mid-flight is out of scope.
