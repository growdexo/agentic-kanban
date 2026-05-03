# Phase 1.5 — Remove Baked Build-Time Secrets

**Phase:** 1 (Safety Hardening)
**Priority:** 🔴 Required before any forked release
**Estimated effort:** 1 day

## Problem

`crates/server/build.rs:6-7` bakes the upstream `POSTHOG_API_KEY` into the binary at build time via `cargo:rustc-env`. Any fork that builds from source inherits the upstream telemetry key and ships data to upstream's PostHog account by default.

Frontend-side, `frontend/src/main.tsx` initializes Sentry with a hard-coded DSN and initializes PostHog from `VITE_POSTHOG_*` build-time env vars. `frontend/src/App.tsx` opts PostHog in/out based on config after startup, but Sentry is initialized before config is loaded.

Backend analytics already uses `option_env!()` plus runtime env fallback in `crates/services/src/services/analytics.rs`; the remaining backend issue is `build.rs` injecting those env vars into the binary. Current config also defaults `analytics_enabled` to `true` in `crates/services/src/services/config/versions/v8.rs`.

Beyond the legal/ethical issue of shipping someone else's telemetry keys, this also violates the PRD privacy requirement that telemetry be off by default.

## Deliverable

1. **Backend:**
   - Remove the `cargo:rustc-env` telemetry key injection from `crates/server/build.rs`.
   - PostHog client initialization becomes conditional: if no key is configured at runtime, disable the client entirely (no-op all calls).
   - Move the source of truth for the key from build-time env to runtime config (env var read at startup, or `AppConfig` field).

2. **Frontend:**
   - Audit `frontend/src/` for `import.meta.env.VITE_*` references that hold telemetry keys. Do not treat unrelated public build settings (`VITE_PARENT_ORIGIN`, React Virtuoso license, remote API base) as telemetry.
   - Same treatment: keys gated by a user-settable opt-in flag in app config; no key → integration disabled.
   - Sentry: `Sentry.init` only called when both a DSN is configured AND user has opted in.
   - PostHog: same pattern.

3. **Default off:**
   - Telemetry opt-in flag in `AppConfig`: `telemetry_enabled: bool` default `false`.
   - Even if a key is present at runtime, no events ship unless the flag is true.
   - Existing `analytics_enabled` defaults/migrations currently treat missing as enabled. Change the default and migration behavior intentionally, and regenerate shared types.

4. **Documentation:**
   - Update README (or `docs/`) to note that fork builds need to set their own keys at runtime if they want telemetry.
   - Document the opt-in flag.

5. **Build cleanup:**
   - Remove or stub the entire `build.rs` env-var dance for telemetry keys.
   - Verify `cargo build` with no env vars set produces a working binary.

## Tests

- Unit test: PostHog client with `None` key is a no-op (no panics, no network calls).
- Manual test: build with no env vars → binary runs, no telemetry initialized, no network calls to PostHog or Sentry on startup or during normal use.
- Manual test: build with env vars set + `telemetry_enabled = false` → still no events sent.
- Manual test: build with env vars set + `telemetry_enabled = true` → events flow.

## Acceptance Criteria

- `cargo build` with no env vars produces a binary that ships zero telemetry by default.
- Network capture on startup of an opt-out build shows no requests to PostHog or Sentry domains.
- Opt-in toggle exists in `AppConfig` (UI surface not required in this task; env var or config file is fine).
- Documentation updated.

## Dependencies

None.

## Notes

- This is a prerequisite for any public release of a forked build under a new identity. Do not skip.
- Sentry's `dsn` is technically a public identifier, not a secret — but it routes events to the upstream account, which is the actual concern. Treat it the same as the PostHog key.
- Consider whether to remove the telemetry libraries entirely from the dependency tree if you don't plan to use them. Smaller binary, smaller attack surface. Out of scope for this task but worth noting.
