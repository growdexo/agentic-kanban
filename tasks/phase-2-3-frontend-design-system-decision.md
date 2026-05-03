# Phase 2.3 — Frontend Design System Decision

**Phase:** 2 (Tech Debt)
**Priority:** 🟡 Blocks Phase 3 UI work
**Estimated effort:** Decision: 1 day. Execution: 1-3 weeks depending on direction.

## Problem

The frontend has two parallel design systems mid-migration:
- `frontend/tailwind.legacy.config.js` + `frontend/tailwind.new.config.js`
- `frontend/components.json` + `frontend/components.legacy.json`
- `frontend/src/components/ui/` + `frontend/src/components/ui-new/`
- `frontend/CLAUDE.md` documents the new design rules; the legacy system is undocumented.

Building Phase 3 UI work (diagnostics page, prompt audit surface, destructive-action confirmation dialogs) on top of an unfinished migration means doubling the work or making the migration permanent.

## Deliverable

This task has two parts: **decide**, then **execute**.

### Part 1: Decide

Pick one direction. The PRD does not care which; it only requires the decision be made and committed.

**Option A — Finish the migration to the new design system.**
- Pros: documented in `frontend/CLAUDE.md`, smaller defaults (8/10/12px font scale), clear tokens, IBM Plex typography, brand orange accent.
- Cons: every existing screen needs to be reviewed and ported; new system uses tighter spacing that may not suit every existing layout.
- Execution effort: 2-3 weeks.

**Option B — Revert to the legacy design system.**
- Pros: stable; most existing screens are already on it.
- Cons: undocumented; throws away the design work already invested in `ui-new/`.
- Execution effort: 1 week (mostly deletion).

### Part 2: Execute

#### If Option A (finish the new system)

- Inventory every screen and component still using `ui/` (legacy).
- Port each to `ui-new/` primitives, preserving behavior.
- Move screens onto the `.new-design` class scope (or remove the scoping if the new system becomes the default).
- Delete `tailwind.legacy.config.js`, `components.legacy.json`, and the entire `ui/` directory.
- Update `components.json` to be the single source of truth.
- Verify Tailwind config has only the new tokens; no legacy color/spacing names.
- Update `frontend/CLAUDE.md` to reflect that the new system is now the only system (remove the `.new-design` scoping note).

#### If Option B (revert to legacy)

- Delete `frontend/src/components/ui-new/`.
- Delete `tailwind.new.config.js`.
- Delete `components.json` (keep `components.legacy.json`, possibly rename to `components.json`).
- Delete `frontend/CLAUDE.md` or replace its content with legacy-system docs.
- Find any screen that already uses `ui-new/` primitives and revert them.
- Verify Tailwind config has only the legacy tokens.

## Constraints

- **No mixed state at the end.** One Tailwind config, one components manifest, one UI primitive directory.
- No regressions in existing screens (visual diffs welcome if intentional; behavior must match).
- Type checks pass: `pnpm run check` and `pnpm run lint`.

## Tests

- Manual visual review of every primary screen: kanban board, task detail, attempt detail, diff view, settings, project create.
- `pnpm run lint` passes with zero warnings (the config sets `--max-warnings 0`).
- `pnpm run check` passes.
- Storybook (if present) renders without errors.

## Acceptance Criteria

- One Tailwind config file.
- One components manifest.
- One UI primitive directory.
- `frontend/CLAUDE.md` reflects the chosen direction (or is deleted if Option B).
- No screen rendering regressions.
- All checks pass.

## Dependencies

None. This is a prerequisite for any Phase 3 UI work, so do it early.

## Notes

- Make the decision before writing any code. Execution is mostly mechanical once the direction is set.
- If unsure: lean Option A. The new system is documented and intentional; reverting wastes the design investment. But if Option A's spacing doesn't fit your real screens, don't force it — Option B is a legitimate choice.
- Consider taking screenshots of every screen before starting and after finishing. Catch unintended visual changes.
