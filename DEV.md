# Dev Setup Notes

These are the local fixes needed to get `npm run dev` / `pnpm run dev` running in this checkout.

## 1. Restore the pnpm workspace file

The frontend dependencies will not install correctly if `pnpm-workspace.yaml` is missing. Without it, pnpm only wires the root package and `frontend/node_modules/vite` can point at a missing store path.

Expected workspace file:

```yaml
packages:
  - frontend
  - remote-frontend
```

This repo's workspace file also contains dependency build allowlists and security overrides. If it is deleted, restore it from git:

```bash
git checkout -- pnpm-workspace.yaml
```

## 2. Reinstall workspace dependencies

After restoring the workspace file, reinstall dependencies from the repo root:

```bash
pnpm install --force
```

If Vite fails with:

```text
Cannot find module '.../frontend/node_modules/vite/bin/vite.js'
```

the local pnpm store/link layout may be corrupt. Repair it with:

```bash
pnpm store prune
pnpm install --force
```

Verify Vite resolves:

```bash
pnpm --dir frontend exec vite --version
```

## 3. Install cargo-watch

The backend dev command uses `cargo watch`:

```bash
cargo install cargo-watch --locked
```

Use `--locked` if your Rust toolchain rejects newer transitive dependencies.

Verify:

```bash
cargo watch --version
```

## 4. Rust toolchain note

This repo has a `rust-toolchain.toml` pin. If Rust commands try to sync that toolchain, let rustup finish installing it.

If you installed Rust 1.88 manually, remember that this repo may still use the toolchain pinned in `rust-toolchain.toml` unless you change the file or set an override.

Check what is active:

```bash
rustup show active-toolchain
rustc --version
cargo --version
```

## 5. Start dev

From the repo root:

```bash
pnpm run dev
```

`npm run dev` also delegates to the same script, but this repo is configured for pnpm.

The dev script:

- allocates frontend/backend ports through `scripts/setup-dev-environment.js`
- starts the Rust backend through `cargo watch`
- starts the frontend through Vite

If the frontend starts but the backend compiles for a long time on first run, that is expected.

