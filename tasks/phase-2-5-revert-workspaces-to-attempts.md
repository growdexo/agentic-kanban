# Phase 2.5 — Revert Workspaces/Sessions Back to Attempts

**Phase:** 2 (Tech Debt)
**Priority:** 🟠 Architecture decision required — do not start as a routine Phase 2 refactor
**Estimated effort:** 2-4 weeks if approved, plus migration/QA buffer

## Current Readiness

This task is **not ready to execute as written**. The codebase has already moved deeply into the Workspace + Session vocabulary: database models, migrations, route modules, generated types, frontend `ui-new` screens, queue/review endpoints, and remote/project sync contexts all refer to workspaces or sessions. Reverting that shape may still be the right product call, but it is not a low-risk tech-debt cleanup.

Before implementation, write and approve an ADR answering:
- Is "Workspace" now a product concept in the new UI, or only an implementation detail that should disappear?
- Are multiple sessions per workspace genuinely unused, or are queue/review/remote flows already relying on the split?
- Is API churn acceptable for all local, remote, MCP, and frontend callers?
- Should this wait until after safety hardening and migration-backup work?

## Problem

The migration `crates/db/migrations/20251216142123_refactor_task_attempts_to_workspaces_sessions.sql` renamed `task_attempts` → `workspaces` and split a new `sessions` table off (one row per workspace). A follow-up `20251221000000_add_workspace_flags.sql` further extended this model.

The `AGENT-KANBAN-PRD.md` model is simpler:
- **Attempt** — one isolated run against a task (the previous `task_attempts`).
- **ExecutionProcess** belongs directly to Attempt and carries agent context (`agent_profile_id`, `prompt_path`, `prompt_snapshot`, etc.).
- **Session** is explicitly deferred to "Expanded Model" — only introduced when multiple agent conversations per attempt become a real need.

The current Workspace + Session split is over-modeled for the actual product behavior. It also confuses the term "workspace" (which the PRD reserves for `workspace_root` — the directory that contains worktrees). The split should be reverted before further feature work.

## Surface Area

Touched by this revert:

**Database (Rust):**
- `crates/db/src/models/workspace.rs` → rename to `attempt.rs`, restore `executor` field.
- `crates/db/src/models/workspace_repo.rs` → rename to `attempt_repo.rs` (or fold into attempt if single-repo per PRD core model).
- `crates/db/src/models/session.rs` → delete (collapse fields into Attempt + ExecutionProcess).
- `crates/db/src/models/execution_process.rs` → swap `session_id` back to `attempt_id`. Add `agent_profile_id` (or equivalent) directly on the row.
- `crates/db/src/models/coding_agent_turn.rs` → audit; PRD collapses AgentTurn into ExecutionProcess. Likely candidate for deletion or merge.
- `crates/db/src/models/merge.rs` → swap `workspace_id` back to `attempt_id`.
- `crates/db/src/models/task.rs` → swap `parent_workspace_id` back to `parent_task_attempt` (or drop entirely if subtasks are in Phase 5 backlog).

**Migrations:**
- New forward migration that undoes the December split. Do **not** edit the existing migrations.
- Migration must preserve all data: every workspace becomes an attempt, every session is folded into its parent attempt's executor field.
- For execution_processes: re-derive `attempt_id` from the current `session_id → workspace_id` join and write it back.
- For merges: re-derive `attempt_id` from `workspace_id`.
- Pre-migration backup (Phase 4.2) is a hard prerequisite if it has shipped; otherwise document the manual backup step in the migration's header comment.

**Services:**
- `crates/services/src/services/workspace_manager.rs` → rename to `attempt_manager.rs` and update internal vocabulary.
- `crates/services/src/services/container.rs` and friends → swap workspace/session terminology to attempt.
- Any service that resolves `session_id → workspace_id` indirection: replace with direct `attempt_id`.

**Routes:**
- `crates/server/src/routes/task_attempts/workspace_summary.rs` → rename file and function.
- `crates/server/src/routes/sessions/` → audit. If the only purpose was the split-off Session entity, delete the directory and fold any genuinely useful endpoints (queue, review) onto the attempt routes.
- `crates/server/src/routes/containers.rs` → check terminology.
- `crates/server/src/middleware/model_loaders.rs` → swap loader names.
- `crates/server/src/error.rs` → swap error variants.
- `crates/server/src/mcp/task_server.rs` → swap MCP-exposed names if any leak the workspace term.

**Generated types:**
- `crates/server/src/bin/generate_types.rs` → swap exported type names.
- Run `pnpm run generate-types` after Rust changes.

**Frontend:**
- Search-and-replace `workspace` → `attempt` across `frontend/src/` and `remote-frontend/src/` (where applicable).
- Hooks, query keys, route paths, component names.
- Generated `shared/types.ts` will update via the type generator.

**HTTP API:**
- Endpoints under `/workspaces/*` rename to `/attempts/*`.
- Deprecated alias period: not required (single-user local app, no external clients depending on the URL shape — the PRD explicitly says external automation APIs are out of scope).

## Constraints

- **Data preservation.** No row loss across the migration. Verify by row count before/after.
- **Single forward migration.** Do not roll back the December migrations; write a new one that restores the prior shape (with any improvements from intervening changes preserved).
- **Vocabulary discipline.** If the ADR approves the rename, "Workspace" survives only for the directory/worktree container concept. Do not use a blanket `rg workspace` acceptance gate until product-level workspace screens/routes have either been renamed or explicitly retained.
- **Sessions deferred, not deleted from spec.** PRD's Expanded Model section keeps Session as a future option. The revert should remove the implementation, not foreclose the concept.

## Tests

- Migration test: load a fixture DB at the post-December schema → run the new migration → verify row counts and key relationships in the restored shape.
- Integration test: create attempt → start execution → stop → delete. End-to-end with the new vocabulary.
- API test: every renamed endpoint responds correctly.
- Frontend smoke: kanban board, task detail, attempt detail all render and function.

## Acceptance Criteria

- ADR approved with explicit decision to revert Workspace + Session naming/modeling.
- No file in `crates/` references `workspace` except for approved filesystem/worktree concepts and any deliberately retained compatibility aliases.
- No frontend file references workspace except for approved filesystem/worktree concepts, retained compatibility aliases, or legacy comments slated for removal.
- `sessions` table no longer exists (or is empty and unreferenced).
- All existing data migrated; row count parity verified.
- All tests pass.
- HTTP routes under `/attempts/*`.
- Generated `shared/types.ts` reflects the renamed types.

## Dependencies

- **Phase 1.1** (path containment) — not blocking, but the path-safety primitive should already exist before this much code churn.
- **Phase 2.1** (split oversized files) — strongly recommended first, otherwise the rename diff in `task_attempts.rs` and `workspace_manager.rs` becomes unreviewable.
- **Phase 4.2** (migration backups) — strongly recommended before running the data migration.

## Notes

- This is the largest single refactor in Phase 2. Land it as a series of PRs:
  1. New migration + DB model renames + tests (no service/route changes; uses temporary aliases).
  2. Service layer rename.
  3. Route + middleware rename.
  4. Frontend rename + type regeneration.
  5. Cleanup: delete temporary aliases, delete `sessions` table, delete `coding_agent_turn` if folded.
- Keep a search-and-replace cheat sheet in the PR description: `workspace_id → attempt_id`, `Workspace → Attempt`, `WorkspaceRepo → AttemptRepo`, `session_id → attempt_id` (where applicable).
- After this lands, update `VIBE-KANBAN-IMPROVEMENTS-PRD.md` entity mapping table to remove the Workspace/Session notes.
- The `sessions` directory under routes (`crates/server/src/routes/sessions/`) appears to host queue and review endpoints. Audit whether those are about the deleted Session entity or about something else (review session = code review session?) before deleting blindly.
