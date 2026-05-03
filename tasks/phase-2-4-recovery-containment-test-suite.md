# Phase 2.4 — Recovery & Containment Test Suite

**Phase:** 2 (Tech Debt)
**Priority:** 🟡 Trust insurance
**Estimated effort:** 3-4 days

## Problem

Even after Phase 1 ships fixes for the dangerous behaviors (path containment, recovery reconciliation, FS API allowlist, log truncation), the codebase has historically lacked automated tests for the most safety-critical paths. Without a regression net, future changes can quietly break the guarantees Phase 1 establishes.

The PRD lists ~17 required automated tests. Phase 1 tasks ship some of these as part of their own deliverables. This task backfills the rest and consolidates them into a coherent safety suite.

## Deliverable

A comprehensive test suite covering each item from the PRD's "Required automated tests" list. Some items are covered by Phase 1 task deliverables; this task ensures all are present and exercised in CI.

### Test inventory

For each item below: confirm a test exists (note which Phase 1 task provides it) or write a new one.

1. **Branch name generation and validation** (new)
   - Slugify a long task title → result is lowercase, ASCII, dash-separated, length-limited.
   - Slugify titles with unicode, emoji, punctuation → ASCII only.
   - Generated branch name passes `git check-ref-format`.
   - Collision handling appends a suffix.

2. **Worktree path containment under `workspace_root`** — covered by Phase 1.1.

3. **Command template validation and rendering** (new)
   - Required template variable missing → render fails closed.
   - Unknown template variable → validation error.
   - Argv rendering preserves arguments with spaces and quotes correctly.
   - Shell rendering only when explicitly configured.

4. **Repo preflight checks** (new)
   - Repo path does not exist → `repo_not_found`.
   - Path exists but is not a git repo → `not_git_repository`.
   - Target branch missing locally and remotely → `target_branch_not_found`.
   - Branch already exists → `branch_already_exists`.
   - Worktree destination already exists → `worktree_create_failed` with appropriate detail.

5. **Task and attempt state transitions** (new)
   - Valid transitions succeed.
   - Invalid transitions (terminal → running) rejected at the model layer.

6. **Execution process lifecycle** (audit existing; add what's missing)
   - Current `ExecutionProcessStatus` has no `queued` variant; rows are created as `running`.
   - `running → completed` happy path.
   - `running → failed` on nonzero exit/start error.
   - `running → killed` on user stop.
   - `completed → running` rejected if the model layer exposes transition validation. If it does not, add validation before writing this test.

7. **Log ordering and replay** (new)
   - Sequence column is monotonic per execution process.
   - Replay from offset N returns lines >= N in order.

8. **Log truncation behavior** — covered by Phase 1.4.

9. **Cancellation state handling** (audit existing; add what's missing)
   - Stop sends graceful termination first.
   - After timeout, escalates to force kill.
   - Stop on already-completed process is idempotent.

10. **Startup recovery for stale `running` processes** — covered by Phase 1.2.

11. **Diff baseline behavior when target branch moves** (new)
    - `base_commit` recorded at attempt creation.
    - Target branch moves after attempt creation → diff still computed against `base_commit`, stale-base warning surfaced.

12. **Large/binary diff truncation** (new)
    - File over per-file cap → diff truncated indicator shown.
    - Binary file → marked changed but content not rendered.
    - Total diff over whole-attempt cap → truncation indicator on summary.

13. **Delete attempt does not remove paths outside `workspace_root`** — covered by Phase 1.1 integration test; verify here.

14. **Local origin/host validation for HTTP requests** (audit existing)
    - Request with valid Origin header → 200.
    - Request with foreign Origin header → 403.
    - Request with missing Origin → 403.
    - Request with mismatched Host header → 403.

15. **Canonical path containment across symlinks** — covered by Phase 1.1 unit test; verify here.

16. **Out-of-band worktree changes before merge** (new)
    - Files modified directly in worktree → diff includes them.
    - Branch changed outside the app → reconciliation runs, warning surfaced.

17. **First-run setup blockers** (new)
    - Missing git executable → setup blocks with `git_executable_not_found`.
    - Missing agent executable → setup blocks with `agent_executable_not_found`.
    - Workspace root not writable → setup blocks with clear error.

## Constraints

- Tests live next to the code they exercise (`#[cfg(test)]` modules) where possible; integration tests in `crates/*/tests/`.
- No flaky tests. If something requires real time or real processes, use deterministic fixtures or short-lived child processes (`sleep 0.1`, etc.).
- Tests run in <60s total in CI.

## Acceptance Criteria

- Every item in the PRD's "Required automated tests" list has at least one corresponding test.
- `cargo test --workspace` passes.
- CI runs the new tests on every PR.
- A test plan document (or comment in this task) maps PRD test items to test functions for future verification.

## Dependencies

- **Phase 1.1, 1.2, 1.3, 1.4** ship most of the foundational tests. This task fills gaps and adds the items not covered by Phase 1.
- **Phase 2.1** (split files) helpful but not required.

## Notes

- The goal is regression insurance, not 100% coverage. Focus on the dangerous paths.
- Manual test scenarios from the PRD (app restart with running execution, target branch deleted, merge conflict, etc.) are *not* in scope here — they belong in a manual QA checklist or a separate end-to-end harness.
- If a test reveals a bug while writing it, fix the bug in a separate commit and reference both in the PR.
