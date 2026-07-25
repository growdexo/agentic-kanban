# Agentic Kanban Improvements PRD

## Framing

This document is the **fork-and-improve plan** for agentic-kanban. The companion document `AGENT-KANBAN-PRD.md` describes the desired product shape; this document describes how to get there from the current Rust + React codebase without rewriting it.

The decision: keep the existing stack (Rust workspace + React/Vite frontend, distributed via npx), and treat the Agent Kanban PRD as a hardening + extension backlog.

## Non-Goals

- Rewriting in another language or framework. The Phoenix/LiveView option in `AGENT-KANBAN-PRD.md` is explicitly not pursued.
- Net-new entities or surfaces in Phase 1. The first phases harden what exists.
- Rebuilding any component that already meets the PRD bar (process termination, origin middleware, diff baseline tracking, worktree creation locking, macOS path normalization).

## Entity Mapping

The Agent Kanban PRD entities map onto existing codebase concepts. Where the current code differs in shape, the difference is noted but not flagged as a defect unless it blocks PRD goals.

| PRD entity | Current location | Notes |
|---|---|---|
| Project | existing project model | matches |
| Repo | existing project repo association | matches |
| AgentProfile | `crates/executors/src/executors/*` | currently per-executor Rust modules; profile-as-DB-row may come later |
| Task | existing task model | matches |
| Attempt | task attempt + workspace | currently split across multiple tables; consolidation is a later refactor |
| ExecutionProcess | `crates/db/src/models/execution_process.rs` | matches |
| ExecutionLog | `crates/db/src/models/execution_process_logs.rs` | currently SQLite JSONL blobs; PRD wants append-only files |
| Merge | existing merge tracking | matches |
| AppConfig | existing config | matches |

A formal entity audit happens in Phase 3.

---

## Phase 1: Safety Hardening Sprint

**Goal:** Close the five highest-risk gaps from the codebase audit before building anything new on top. Each item ships as its own PR with tests.

**Sized:** ~1-2 weeks for one developer.

### 1.1 Path Containment Primitive

**Problem:** `worktree_manager.rs:267` and `workspace_manager.rs:373` call `remove_dir_all()` without an explicit canonical-path containment check against `workspace_root`. A malformed `container_ref` could in principle delete arbitrary paths.

**Deliverable:**
- New module `crates/utils/src/path_safety.rs` exporting `assert_path_under_root(path, root) -> Result<CanonicalPath, PathSafetyError>`.
- Uses `dunce::canonicalize` on both inputs, then `strip_prefix`.
- Rejects paths whose canonical form escapes the root (including via symlinks).
- All `remove_dir_all` and `rename` calls on attempt/worktree paths route through this primitive.
- Unit tests for: relative paths, symlinks pointing outside root, `..` traversal, case-sensitivity edge cases on macOS, non-existent paths, paths exactly at root.

**Acceptance:** No `remove_dir_all` call in `crates/services` or `crates/local-deployment` runs without first calling `assert_path_under_root`. Verified by a grep-based CI check.

### 1.2 Process Recovery Reconciliation

**Problem:** `crates/services/src/services/container.rs:257-307` marks all `running` execution processes as `failed` on startup without checking PIDs or attempting to reattach.

**Deliverable:**
- Startup recovery scans `running` execution processes.
- For each, check if the recorded PID exists and matches the recorded command (basic `proc` inspection on Linux/macOS).
- If PID is alive and matches: re-attach status as `running`, schedule a re-stream attempt, log a recovery event.
- If PID is dead or mismatched: transition to `killed` with a `recovery_orphaned` reason code.
- Persist a `startup_recovery_summary` record visible on the diagnostics page (see Phase 3).
- Tests: simulated stale PID, simulated alive matching PID, simulated alive mismatched PID.

**Acceptance:** Restarting the app while an execution is running does not silently lose the process or its logs. Restart while execution has died produces `killed` (not `failed`) with a clear reason.

### 1.3 Filesystem API Allowlist

**Problem:** `crates/server/src/routes/filesystem.rs:19-38` accepts arbitrary paths and only checks `exists + is_dir`. Any browser request that passes origin middleware can list `/etc`, `/root`, etc.

**Deliverable:**
- Filesystem listing routes accept paths only if they canonicalize to a location under one of:
  - the user's home directory
  - any registered repo path
  - the configured workspace root
  - the app data directory
- Returns 403 with a clear error code on out-of-allowlist paths.
- The allowlist is computed once per request from durable config.
- Tests: traversal attempts, symlink escapes, valid paths inside each allowlisted root.

**Acceptance:** A scripted browser request for `/etc/passwd` (or its directory) returns 403. Requests for legitimately registered repo paths still succeed.

### 1.4 Log Storage and Truncation

**Problem:** `crates/db/src/models/execution_process_logs.rs:51-68` appends unbounded JSONL rows to SQLite with no size cap, no truncation marker, and no backpressure.

**Deliverable:**
- Configurable per-execution log byte cap (`AppConfig.max_log_bytes_per_execution`, default e.g. 50MB).
- Once cap is hit: append a single `system`-stream truncation marker line, stop accepting further log writes for that execution, and mark the execution with `log_truncated: true`.
- Backpressure: log writes batched (e.g. 100 lines or 100ms, whichever first) before DB insert.
- Tests: cap enforcement, truncation marker presence, batching under burst load.
- Defer the "logs as append-only files instead of SQLite" architectural change to Phase 3 or later. Truncation is the urgent fix.

**Acceptance:** A runaway agent producing 1GB of output does not balloon the SQLite database, does not freeze the UI, and produces a clear truncation indicator.

### 1.5 Remove Baked Build-Time Secrets

**Problem:** `crates/server/build.rs:6-7` bakes `POSTHOG_API_KEY` into the binary at build time via `rustc-env`.

**Deliverable:**
- Replace `env!("POSTHOG_API_KEY")` usage with `option_env!()` or a runtime config lookup.
- Telemetry off by default unless the user has explicitly opted in (matches `AGENT-KANBAN-PRD.md` privacy section).
- Document that fork builds need to set their own key (or none) at runtime.
- Frontend: same treatment for `posthog-js` and `@sentry/react` — gated by user-configured opt-in flags rather than baked keys.

**Acceptance:** A build with no env vars set produces a binary that ships zero telemetry by default. Opt-in toggles are visible in app config.

---

## Phase 2: Tech Debt Sprint

**Goal:** Reduce structural friction so Phase 3 work is pleasant. Pure refactor; no behavior change.

**Sized:** ~2-3 weeks.

### 2.1 Split Oversized Files

- `crates/executors/src/executors/claude.rs` (2,709 lines) → split by concern: lifecycle, prompt construction, output normalization, auth detection.
- `crates/server/src/routes/task_attempts.rs` (2,002 lines) → split by route group: lifecycle, diff, merge, branch ops, follow-up.

**Acceptance:** No file in `crates/executors` or `crates/server/src/routes` exceeds 800 lines. All tests still pass.

### 2.2 Consolidate `normalize_logs` Duplication

- Four near-identical copies (`claude/normalize_logs.rs`, `codex/normalize_logs.rs`, `droid/normalize_logs.rs`, `opencode/normalize_logs.rs`), ~4,700 LOC total.
- Extract a `LogNormalizer` trait in `crates/executors/src/normalize.rs` with shared parsing primitives.
- Each executor implements only the executor-specific format quirks.

**Acceptance:** Net LOC reduction of >50% across the four files. A bug fix to log parsing requires touching one place, not four.

### 2.3 Frontend Design System Decision

**Problem:** Frontend has two parallel design systems mid-migration (`tailwind.legacy.config.js` + `tailwind.new.config.js`, `components.json` + `components.legacy.json`, `ui-new/` directory).

**Deliverable:** Pick one of:
- **(a)** Finish the migration to the new design system. Delete `tailwind.legacy.config.js`, `components.legacy.json`, and any `ui/` components superseded by `ui-new/`.
- **(b)** Revert to the legacy system. Delete `ui-new/`, `tailwind.new.config.js`, and the `frontend/CLAUDE.md` new-design rules.

The choice is the user's; this PRD just requires the decision be made and executed before Phase 3 UI work begins.

**Acceptance:** One Tailwind config, one components manifest, one UI primitive directory.

### 2.4 Recovery & Containment Test Suite

Even if Phase 1 fixes ship without regressions, the codebase has no automated tests for the most dangerous behaviors. Backfill:

- Worktree path containment under `workspace_root` (Phase 1.1 already ships unit tests; add integration tests that exercise the full delete flow).
- Startup recovery for stale `running` processes (Phase 1.2 fixtures).
- Symlink traversal blocking on filesystem API (Phase 1.3 fixtures).
- Log truncation behavior (Phase 1.4 fixtures).
- Branch name generation and validation against `git check-ref-format`.
- Diff baseline behavior when target branch moves.
- Out-of-band worktree changes before merge.

**Acceptance:** All items in the PRD's "Required automated tests" list have at least one corresponding test.

---

## Phase 3: Discipline Gaps

**Goal:** Bring the existing app up to the PRD's discipline bar in places where current behavior is incomplete.

**Sized:** ~3-4 weeks.

### 3.1 Diagnostics Page

The PRD calls for a local diagnostics page early because most failures are environment-specific. Currently scattered or absent.

**Deliverable:** New route + view showing:
- App version, app data directory, workspace root, SQLite path
- Detected git version, shell, agent profiles + availability
- Configured editor command, max concurrency, log limits
- Active execution processes
- Recent failed execution processes (last 50)
- Orphan worktrees owned by the app
- Last startup recovery summary (from Phase 1.2)

Actions: copy diagnostics summary, open app data dir, open workspace root, rerun agent availability checks, run git health check for a repo.

**Acceptance:** A user can describe their environment and recent failures by pasting one diagnostics dump.

### 3.2 Destructive Action Confirmation Hardening

The PRD requires confirmation copy that names affected resources and indicates whether the operation touches files outside the app data directory.

**Deliverable:**
- Audit every destructive endpoint (`delete_workspace`, `delete_attempt`, `delete_branch`, `force_push`, `merge`, `reset_app_metadata`).
- Server-side: each accepts a confirmation payload that explicitly names the affected branch and/or path; mismatch returns 409 with the canonical name the user must confirm.
- Client-side: confirmation dialogs surface affected repo, branch, filesystem path, undo-ability, and whether the operation touches files outside the app data dir.

**Acceptance:** No destructive endpoint can be triggered without an explicit name match in the request body. CSRF-style replay attacks fail.

### 3.3 Prompt Audit Surface

The PRD requires the final prompt payload (or prompt-file path) to be persisted with the execution process and visible in the UI before first run and after.

**Deliverable:**
- `ExecutionProcess` gains `prompt_path` and/or `prompt_snapshot` columns (matches PRD entity).
- Backfill where feasible; new executions populate from day one.
- UI surface: each execution process detail panel shows the exact rendered command, cwd, env snapshot (with secret redaction), and prompt content/path.
- Pre-run preview: starting an attempt shows the rendered command + prompt before user confirms.

**Acceptance:** Every execution process record has an inspectable prompt artifact. First-run of any agent profile shows the rendered command before execution.

### 3.4 Failure Code Taxonomy

The PRD lists ~20 required failure codes. Current code has scattered error types.

**Deliverable:**
- New enum `FailureCode` in `crates/utils` covering the PRD list.
- Every user-facing failure path returns a `FailureCode` plus user-facing message, technical detail, suggested next action, and related entity ids.
- Frontend renders code → message → action consistently.

**Acceptance:** Every failure surfaced in the UI has a stable error code that maps to actionable copy.

### 3.5 Logs as Append-Only Files (Architectural)

Once Phase 1.4 truncation is in place, revisit storage architecture:

- Move large log bodies out of SQLite into append-only files under app data dir.
- SQLite stores execution metadata + offset checkpoints.
- LiveView replay reads from file with offset-based incremental delivery.

This is the larger architectural change deferred from Phase 1. Only attempt after Phase 1.4 has been in production long enough to confirm the cap behavior is correct.

**Acceptance:** SQLite size stops growing linearly with log volume. Log replay after reload is incremental.

---

## Phase 4: Distribution & Trust Cleanup

**Goal:** Make a forked build trustworthy to ship under a new identity.

**Sized:** ~1 week.

### 4.1 Telemetry Strip / Replace

Phase 1.5 removes baked secrets. Phase 4.1 finishes the job:
- Remove or replace PostHog and Sentry integrations entirely if not needed.
- If retained: opt-in only, with a visible toggle in app config and a redaction policy that excludes source code, prompts, logs, diffs, repo paths, branch names, task descriptions, env values (matches PRD privacy section).

### 4.2 Migration & Backup

PRD requires backups before destructive migrations.

**Deliverable:**
- On startup, before running pending migrations, snapshot the SQLite DB to `app_data/backups/{timestamp}.db`.
- Configurable retention (default: keep last 5).
- Failed migration halts startup with a diagnostic message and a pointer to the backup.

### 4.3 Localhost-Only by Default

Audit `crates/server/src/main.rs:102`: confirm default bind is `127.0.0.1`. If user configures non-localhost bind, log a warning at startup that local repos and command execution become network-reachable.

### 4.4 Editor Command Policy

PRD says editor commands must be user-configured, previewed before first use, and stored in app config. Audit the current editor-handoff flow against this rule and tighten as needed.

---

## Phase 5: Optional Surfaces (Backlog)

These match the PRD's "Optional Surfaces" section. Tackle individually only after Phases 1-4 are done and only when there is a concrete user need.

- Multiple attempts per task with `parent_attempt_id` (subtasks)
- Follow-up prompts that resume agent context
- Setup and cleanup scripts as first-class profile fields
- Branch push (already partially exists; harden against PRD destructive-action rules)
- PR creation through GitHub CLI (already exists; audit against trust boundary)
- Attempt archive/pin polish
- Multi-repo attempts (introduce `AttemptRepo` junction at this point)
- Subtasks linked to parent attempts
- Preview / dev server panel
- Inline diff comments
- Rebase / conflict workflows
- Persistent terminal (PTY mode)
- Import tasks from markdown / specs

---

## Out of Scope

- MCP integration changes. Current MCP surface stays as-is unless a separate decision is made. Note: the Agent Kanban PRD explicitly says external automation APIs are out of scope; this fork retains MCP rather than removing it, on the grounds that removing a working feature is not an improvement.
- Native Windows support. WSL-only matches PRD.
- Multi-user / cloud / remote deployment changes. The `crates/remote` and `remote-frontend` workspaces are out of scope for this PRD.
- Generic kanban features (labels, swimlanes, custom columns) beyond what already exists.
- Visual redesign beyond the migration decision in Phase 2.3.

---

## Acceptance Criteria for the Fork as a Whole

After Phase 1 ships:
- No deletion path can escape `workspace_root`.
- App restarts do not silently lose `running` execution state.
- Filesystem API cannot list arbitrary paths.
- Runaway logs do not balloon the database or freeze the UI.
- Builds contain no baked third-party secrets.

After Phase 2 ships:
- No source file exceeds 800 lines.
- Log normalization is one place, not four.
- Frontend has one design system.
- Dangerous behaviors have automated test coverage.

After Phase 3 ships:
- Diagnostics page exists and explains environment + recent failures.
- Every destructive endpoint requires explicit name match.
- Every execution process has an inspectable prompt artifact.
- Every failure has a stable code and actionable copy.

After Phase 4 ships:
- The fork can be distributed under a new identity without leaking the upstream's telemetry keys.
- Migrations are reversible via backup.
- Default bind is localhost; non-localhost binds warn loudly.

---

## Sequencing & Risk Notes

- Phase 1 items can ship in parallel — they touch different files. Phase 1.1 (path containment) is the prerequisite for any Phase 5 work that involves deletion.
- Phase 2.1 (file splits) should ship before Phase 3 to keep diff sizes manageable in later route work.
- Phase 2.3 (design system decision) blocks any Phase 3 UI work. Make the call early.
- Phase 3.5 (logs as files) is the largest architectural change in this document. Treat it as its own multi-week project. Do not start until Phase 1.4 is verified in production.
- Phase 4.1 (telemetry) is required before any public release of a forked build under a new name; legally and ethically you cannot ship someone else's telemetry keys.
