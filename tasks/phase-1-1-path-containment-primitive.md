# Phase 1.1 — Path Containment Primitive

**Phase:** 1 (Safety Hardening)
**Priority:** 🔴 Highest — blocks any Phase 5 deletion work
**Estimated effort:** 1-2 days

## Problem

`crates/services/src/services/worktree_manager.rs:267` and `crates/services/src/services/workspace_manager.rs:373` call `std::fs::remove_dir_all()` on attempt/worktree paths without an explicit canonical-path containment check against `workspace_root`. A malformed `container_ref`, a symlink, or a future code path that forgets the implicit assumption could in principle delete arbitrary user files.

The containment logic is currently scattered across creation and cleanup paths rather than centralized in a tested primitive.

## Deliverable

1. **New module:** `crates/utils/src/path_safety.rs`
   - Public function: `assert_path_under_root(path: &Path, root: &Path) -> Result<CanonicalPath, PathSafetyError>`
   - Uses `dunce::canonicalize` on both inputs. `dunce` currently exists in `crates/services/Cargo.toml`; add it to `crates/utils/Cargo.toml` as part of this task.
   - Performs `strip_prefix` on canonical forms.
   - Returns a newtype `CanonicalPath` so callers cannot accidentally re-use the un-validated input.
   - Error variants: `PathDoesNotExist`, `RootDoesNotExist`, `PathEscapesRoot`, `CanonicalizeFailed`.

2. **Caller migration:**
   - Every `remove_dir_all` call in `crates/services` and `crates/local-deployment` that targets an attempt/worktree path routes through `assert_path_under_root` first.
   - Do not mechanically wrap unrelated operations such as config/profile backup renames or test fixture cleanup; those need their own targeted safety checks if they ever become user-controlled.
   - Callers receive a `CanonicalPath` and pass that into the destructive op.

3. **CI guard:**
   - Add a grep-based check (script in `scripts/` or a clippy lint config) that fails if a new attempt/worktree deletion path calls `remove_dir_all` without the helper.
   - Keep an explicit allowlist for unrelated existing calls (`qa_repos`, OAuth/config backup paths, temporary staging cleanup) so the guard is precise rather than noisy.

## Tests

Unit tests in `crates/utils/src/path_safety.rs`:
- Path is a direct child of root → ok.
- Path is a deep descendant of root → ok.
- Path is exactly the root → ok or rejected (pick one and document).
- Path is a sibling of root → rejected.
- Path uses `..` to escape root → rejected after canonicalization.
- Symlink inside root pointing outside root → rejected after canonicalization.
- Relative path → rejected or canonicalized against cwd (pick one and document).
- Non-existent path → `PathDoesNotExist`.
- Non-existent root → `RootDoesNotExist`.
- macOS `/private/var` vs `/var` aliasing → ok (verify the existing `path.rs` normalization is used consistently).
- Case-sensitivity edge cases on case-insensitive filesystems (macOS default) → documented behavior.

Integration test:
- End-to-end attempt deletion: create attempt under workspace root, delete via the public API, verify worktree removed and no files outside root touched.
- Attempt deletion with a symlinked worktree path pointing outside root → operation is refused. If product behavior later requires deleting the symlink entry itself, implement that as a separate `lstat` + unlink path; do not make canonical containment silently allow it.

## Acceptance Criteria

- No attempt/worktree deletion in `crates/services` or `crates/local-deployment` runs without first obtaining a `CanonicalPath` from `assert_path_under_root`.
- CI fails on any new direct attempt/worktree deletion path in those crates.
- All listed tests pass.
- A manual test where `container_ref` is mutated to point outside `workspace_root` results in a refused operation with a clear `FailureCode::PathEscapesRoot` (or equivalent), not a silent deletion.

## Dependencies

None. This is the foundation task.

## Notes

- Do not depend on `dunce` already being canonicalized at insertion time. Always canonicalize at the boundary.
- This primitive should be the *only* sanctioned way to validate destructive paths going forward. Document that in the module doc comment.
