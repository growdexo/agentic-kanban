# Phase 2.2 — Consolidate `normalize_logs` Duplication

**Phase:** 2 (Tech Debt)
**Priority:** 🟡 High-leverage refactor
**Estimated effort:** 3-5 days

## Problem

The normalization duplication is real, but the current file layout is slightly different from the initial audit:
- Claude normalization currently lives inline in `crates/executors/src/executors/claude.rs` (there is no `crates/executors/src/executors/claude/normalize_logs.rs` yet).
- `crates/executors/src/executors/codex/normalize_logs.rs`
- `crates/executors/src/executors/droid/normalize_logs.rs`
- `crates/executors/src/executors/opencode/normalize_logs.rs`
- `crates/executors/src/executors/acp/normalize_logs.rs` is smaller but should be audited for shared primitives too.

The standalone files plus ACP total ~4,500 LOC, and Claude adds another large inline normalizer inside a 2,709-line file. Any bug fix or feature addition has to be replicated across multiple executor-specific paths. Adding another executor risks another full copy.

## Deliverable

1. **Extract a `LogNormalizer` trait** in `crates/executors/src/normalize.rs`:
   - Defines the shared pipeline: chunk → parse JSONL → classify → enrich → emit.
   - Provides default implementations for the steps that are genuinely identical across executors.
   - Exposes hook points (associated functions or trait methods) for executor-specific behavior.

2. **Extract shared primitives** into `crates/executors/src/normalize/` submodules:
   - `jsonl_parse.rs` — line buffering, JSON parsing, partial-line handling.
   - `ansi.rs` — any shared ANSI handling (currently duplicated).
   - `classify.rs` — common stream classification (stdout / stderr / system).
   - `redact.rs` — secret-pattern redaction (if currently shared).

3. **Per-executor implementations** become small:
   - Each executor's `normalize_logs.rs` shrinks to its actual unique logic: format-specific parsing, executor-specific event types, custom enrichment.
   - For Claude, Phase 2.1 should first split `claude.rs`; then move its inline normalizer into `crates/executors/src/executors/claude/normalize_logs.rs` before extracting shared pieces.
   - Target: each executor's normalizer file under 300 lines.

4. **Tests:**
   - Move shared test cases to `crates/executors/src/normalize/tests.rs`.
   - Each executor keeps tests for its unique parsing logic.
   - Add property tests for the shared parser if reasonable (e.g., partial-line reassembly).

## Constraints

- **No behavior change visible in the UI.** Same log lines render the same way before and after.
- All existing tests for each executor pass with the new structure.
- Adding a fifth executor should require <300 lines of new normalizer code.

## Tests

- Existing per-executor normalization tests pass without modification (or with mechanical updates only).
- New shared-primitive tests cover the extracted pipeline.
- Manual smoke: run an attempt with each of the four executors, verify log rendering looks identical to pre-refactor.

## Acceptance Criteria

- Net LOC reduction in `crates/executors/src/executors/*/normalize_logs.rs` of at least 50%.
- A bug fix to JSONL parsing or ANSI handling requires editing one file, not four.
- All existing tests pass.
- A grep for `fn normalize` in the executor crate shows the shared trait being implemented, not re-implemented.

## Dependencies

- **Phase 2.1** should ideally land first to keep diff sizes manageable, but not strictly required.

## Notes

- Start by diffing the four files against each other to identify the actual variation surface. The genuine differences are likely smaller than the visual size suggests — a lot of the bulk is probably formatting.
- If the variation is large enough that the trait abstraction becomes leaky, prefer composition (shared helper functions called from each executor) over forcing a trait. The goal is dedup, not enforced uniformity.
- This is a high-leverage refactor: the next time you add an executor or fix a parsing bug, it pays for itself.
