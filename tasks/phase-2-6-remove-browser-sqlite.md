# Phase 2.6 — Remove In-Browser SQLite (Electric / wa-sqlite)

**Phase:** 2 (Tech Debt)
**Priority:** 🟡 Simplifies frontend; reduces bundle size and complexity
**Estimated effort:** 3-5 days

## Problem

The frontend ships an in-browser SQLite stack:
- `wa-sqlite` (WebAssembly SQLite build) — frontend dependency.
- `@tanstack/electric-db-collection` + `@tanstack/react-db` — TanStack DB primitives bound to ElectricSQL shapes.
- `frontend/src/lib/electric/collections.ts` (358 lines) and `hooks.ts` (260 lines) wire it up.

For purely local data where the server is on `localhost`, this is probably overkill:
- The server already speaks SQLite directly. There is no network latency to mask.
- Optimistic local state and shape-based sync add complexity without proportional UX wins for localhost requests.
- `@tanstack/react-query` (already a dependency) handles fetching, caching, and invalidation cleanly with plain HTTP.
- `wa-sqlite` adds ~1 MB to the bundle and meaningful boot cost.

However, the current Electric usage is tied to authenticated remote shapes (`REMOTE_API_URL`, `shared/remote-types`, organization/project/issue contexts), not just local localhost state. Existing local WebSocket streams do not automatically replace those remote Electric subscriptions.

The goal should be one of two explicit choices:
- **Option A:** Remote sync remains in scope. Isolate Electric behind remote-only hooks/routes and remove it from local screens/bundles where possible.
- **Option B:** Remote sync is out of scope for this fork. Remove the Electric layer and replace every remote shape usage with plain authenticated HTTP + React Query polling or a new remote event stream.

## Surface Area

**Direct deletions (Option B only):**
- `frontend/src/lib/electric/collections.ts`
- `frontend/src/lib/electric/hooks.ts`
- `frontend/src/lib/electric/types.ts`
- Anywhere `wa-sqlite` is initialized or imported.

**Dependency removal (`frontend/package.json`, Option B only):**
- `@tanstack/electric-db-collection`
- `@tanstack/react-db`
- `wa-sqlite`

Run `pnpm install` after removal to update the lockfile.

**Remote-only isolation (Option A only):**
- Keep Electric dependencies, but move Electric imports behind remote-only route boundaries so local app screens do not pay the bundle/runtime cost.
- Add bundle analysis showing the local entry no longer eagerly includes `wa-sqlite` / Electric code.

**Caller migration:**
- Every component or hook that calls `useCollection`, `useLiveQuery`, or imports from `@/lib/electric/*` switches to:
  - `useQuery` / `useMutation` from `@tanstack/react-query` for HTTP-backed reads and writes.
  - Existing local WebSocket subscription for local state invalidation, or a newly-defined remote invalidation mechanism for remote shapes.
- If Option B is chosen, run `grep -rln "electric\|useCollection\|useLiveQuery\|wa-sqlite" frontend/src/` after migration; result must be empty.
- If Option A is chosen, the grep may still find remote-only modules, but no local route/screen should import them.

**Realtime invalidation:**
- The current Electric flow auto-syncs on shape changes. After the rip, the app needs a deterministic way to invalidate React Query caches when server state changes.
- Use the existing WebSocket event stream for local app state. For remote org/project/issue state, first add or identify an equivalent remote event stream; do not assume the local event stream covers it.
- Centralize this in a single hook (e.g., `useServerEventSync`) mounted at the app root.

**Auth:**
- `collections.ts` references `tokenManager` from `'../auth/tokenManager'`. If that exists only to feed the Electric fetch wrapper, audit whether it's still needed or can be simplified. (Local single-user app — token management may be vestigial.)

**Remote frontend:**
- `remote-frontend/` may have its own copy or its own usage. Out of scope for this task per `VIBE-KANBAN-IMPROVEMENTS-PRD.md` non-goals (the remote deployment is out of scope). If it imports from `frontend/src/lib/electric`, decouple before removing the local copy.

## Constraints

- **No regression in realtime behavior.** Log streams, status updates, and diff refreshes must still feel live after the rip.
- **No regression in optimistic UX where it genuinely matters.** Most local API calls return in <50ms; optimistic updates rarely add value. If a specific interaction (e.g., drag-and-drop reordering) feels worse without optimism, use React Query's `onMutate` + `setQueryData` for targeted optimistic updates.
- **One source of truth for server data.** After the rip, every component reads from React Query caches. No parallel state stores syncing the same data.
- All checks pass: `pnpm run check`, `pnpm run lint`.

## Tests

- Manual smoke: load each primary screen, verify data renders.
- Manual smoke: trigger server-side state change (start an attempt) → verify UI updates without page reload via WebSocket invalidation.
- Manual smoke: run a long agent execution → verify log streaming still feels live.
- Bundle size measurement before and after — record the delta in the PR description.

## Acceptance Criteria

- Decision recorded: Option A remote-only isolation or Option B full removal.
- For Option B: `wa-sqlite`, `@tanstack/electric-db-collection`, `@tanstack/react-db` removed from `frontend/package.json`; `frontend/src/lib/electric/` deleted; grep for Electric usage in `frontend/src/` returns empty.
- For Option A: local app routes no longer load Electric code, and remote routes remain functionally equivalent with measured bundle impact.
- All primary screens render and update correctly.
- Realtime updates (logs, status, diff) still flow.
- Bundle size reduced or code-split impact measured; record the actual gzip/brotli delta rather than assuming an 800 KB gzipped win.
- All checks pass.

## Dependencies

- **Phase 2.3** (frontend design system decision) — unrelated but both touch the frontend; sequencing matters only to avoid merge conflicts.
- **Phase 2.5** (revert workspaces) — if it lands first, the frontend rename and the Electric rip can be combined into one frontend sweep. Otherwise sequence them so the rename happens before or after, not during.

## Notes

- The Electric stack is not bad technology — it's simply the wrong tool for a localhost app. Don't frame the rip as "Electric is broken"; frame it as "we don't need shape-based sync when the server is one hop away."
- Watch for places that depended on Electric's automatic optimistic behavior. Most of those will be invisible (the user expects the change to land instantly because the call is local), so plain `useMutation` with cache invalidation is fine. The few that genuinely need optimism can use React Query's optimistic patterns explicitly.
- After the rip, the `frontend/src/lib/auth/tokenManager.ts` may also be vestigial. Audit and simplify in a follow-up — out of scope here unless the audit reveals it's only used by Electric.
- If `remote-frontend/` shares the Electric code, that's its own decision: the remote deployment may genuinely benefit from shape-based sync (real network latency). Don't remove it there reflexively.
