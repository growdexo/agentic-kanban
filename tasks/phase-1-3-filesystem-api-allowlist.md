# Phase 1.3 — Filesystem API Allowlist

**Phase:** 1 (Safety Hardening)
**Priority:** 🔴 High
**Estimated effort:** 1-2 days

## Problem

`crates/server/src/routes/filesystem.rs:19-38` accepts arbitrary paths as a query parameter and only checks `exists + is_dir` via `crates/services/src/services/filesystem.rs:316-364`. Any browser request that passes the origin middleware can list `/etc`, `/root`, or any other directory the server process can read.

Localhost binding mitigates blast radius but does not fix the underlying issue. A malicious page in any browser tab on the user's machine that can issue a CSRF-bypassing request, or any future feature that loosens origin checks, exposes the filesystem.

## Deliverable

1. **Allowlist construction** in `crates/services/src/services/filesystem.rs`:
   - New function `compute_browse_allowlist(config, projects) -> Vec<CanonicalPath>` returning canonical roots a user can browse:
     - The user's home directory (`dirs::home_dir`).
     - The configured `workspace_root`.
     - The app data directory.
     - Every registered repo path across all projects.
   - All entries canonicalized via the Phase 1.1 primitive.

2. **Path validation** on every filesystem listing call:
   - Canonicalize the requested path.
   - Reject if it does not lie under at least one allowlist entry.
   - Reject symlinks whose canonical target escapes all allowlist entries.

3. **Error response:**
   - Return HTTP 403 with `FailureCode::PathNotAllowed` (or equivalent) and a message naming which roots *are* allowed (so legitimate users get useful feedback, but with no path traversal hint).
   - Current `routes/filesystem.rs` wraps service errors in `ApiResponse::error(...)` with a 200 response. Change the route return path to emit a real 403 status for allowlist failures while preserving existing 200 success responses.

4. **Caller updates:**
   - `routes/filesystem.rs` list/browse endpoints validate before serving.
   - Any other endpoint that accepts a user-supplied path for read access (repo picker, file viewer if any) goes through the same gate.

## Tests

Unit tests in `crates/services`:
- `compute_browse_allowlist` returns canonical paths for home, workspace, app data, and registered repos.
- Allowlist excludes a path that is not registered.

Integration tests in `crates/server`:
- GET filesystem list for `/etc` → 403.
- GET filesystem list for `/etc/passwd` parent → 403.
- GET filesystem list for the user's home dir → 200.
- GET filesystem list for a registered repo path → 200.
- GET filesystem list for a path with `..` traversal trying to escape an allowed root → 403 after canonicalization.
- GET filesystem list for a symlink inside an allowed root that points to `/etc` → 403 after canonicalization.

## Acceptance Criteria

- A scripted browser request (with valid origin) for `/etc/passwd`'s parent returns 403.
- Requests for legitimately registered repo paths and the user's home dir still succeed.
- No regression in the repo picker or any existing UI that browses the filesystem.
- All listed tests pass.

## Dependencies

- **Phase 1.1** (path containment primitive) — uses `assert_path_under_root` for the actual containment check.

## Notes

- Do *not* allow arbitrary subdirectory traversal once a root is matched — canonicalize the full requested path and check it is under one allowlist entry. Don't just check the prefix matches the requested string.
- The home directory is wide. If that ends up being too permissive in practice, narrow it to a configurable `browse_roots` list later. Out of scope for v1 of this task.
