# Phase 2.1 — Split Oversized Files

**Phase:** 2 (Tech Debt)
**Priority:** 🟡 Blocks Phase 3 route work
**Estimated effort:** 2-3 days

## Problem

Two files dominate their crates and mix unrelated concerns, making PRD-compliance audits and feature work in those areas painful:

- `crates/executors/src/executors/claude.rs` — **2,709 lines**. Mixes lifecycle, prompt construction, output normalization, auth detection, and command rendering.
- `crates/server/src/routes/task_attempts.rs` — **2,002 lines**. Mixes lifecycle endpoints, diff endpoints, merge endpoints, branch operations, and follow-up endpoints.

Both files are change-magnets and will fight every Phase 3 deliverable.

Current audit note: there are other >800-line files (`opencode/sdk.rs`, `cursor.rs`, executor normalizers, and `routes/task_attempts/pr.rs`). This task is scoped to the two worst change-magnets above. Do not use a global "no file over 800 lines" gate until those other files are either split in follow-up tasks or explicitly exempted.

## Deliverable

### claude.rs split

Target structure under `crates/executors/src/executors/claude/`:
- `mod.rs` — public re-exports + `Executor` trait impl.
- `lifecycle.rs` — spawn, monitor, terminate.
- `prompt.rs` — initial and follow-up prompt construction.
- `command.rs` — argv rendering and template variable substitution.
- `auth.rs` — auth detection and CLI availability check.
- `normalize_logs.rs` — move Claude's inline normalization code here mechanically, but leave deduplication/shared abstractions for Phase 2.2.

### task_attempts.rs split

Target structure under `crates/server/src/routes/task_attempts/`:
- `mod.rs` — router assembly + shared helpers.
- `lifecycle.rs` — create, start, stop, archive, delete.
- `diff.rs` — diff payloads, file status.
- `merge.rs` — merge, push, force-push, PR creation.
- `branch.rs` — branch operations (rename, delete, checkout).
- `follow_up.rs` — follow-up prompt + execution.

## Constraints

- **Pure refactor.** No behavior change, no API change, no DB change.
- All existing tests pass without modification.
- Each new file under 800 lines.
- No new public types unless splitting requires them; prefer `pub(crate)` and `pub(super)`.
- Imports grouped per `rustfmt.toml`.
- One PR per file split (i.e., two PRs total) to keep review tractable.

## Tests

- `cargo test --workspace` passes before and after with identical results.
- `pnpm run check` and `pnpm run backend:check` pass.
- Manual smoke: start an attempt with the Claude executor, observe normal log streaming, stop it cleanly. (Catches accidental visibility regressions.)

## Acceptance Criteria

- `crates/executors/src/executors/claude.rs` no longer exists as a monolithic 2,700-line file; its replacement modules are each under 800 lines.
- `crates/server/src/routes/task_attempts.rs` no longer exists as a monolithic 2,000-line file; its replacement modules are each under 800 lines.
- Any remaining >800-line files in `crates/executors/src/executors/` or `crates/server/src/routes/` are listed as follow-up tasks or explicit exemptions.
- All tests still pass.
- Diff is mechanical: `git log -p` on the split commits shows mostly cut-and-paste with import adjustments.

## Dependencies

None functional. Phase 3 work (diagnostics page, destructive action hardening, prompt audit, failure codes) lives mostly in `task_attempts.rs` today, so doing this first makes those PRs much smaller.

## Notes

- Resist the urge to "clean up while you're in there." Pure mechanical split. Cleanup is a separate task.
- If a function genuinely belongs in two new files, leave it in the most-used location and `pub(super)` it for the other; don't extract a third "shared" file unless three or more callers exist.
- Watch for `#[cfg(test)]` blocks — they should travel with the code they test.
