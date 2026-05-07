# Repository Layout

This repo keeps the root thin. Start here:

1. `README.md`
2. `docs/REPO_LAYOUT.md`
3. `scripts/package/check-package-readiness.mjs`

## Root

- `package.json` for npm entrypoints.
- `index.html` for the Vite app shell.
- `src/` for the React coordinator.
- `src-tauri/` for the Rust backend and Tauri config.
- `tests/` for contract checks.

## Script Groups

- `scripts/dev/` for launchers.
- `scripts/setup/` for dependency/bootstrap helpers.
- `scripts/runtime/` for bundled ASR runtime setup and the Tauri wrapper.
- `scripts/package/` for packaging checks and bundle verification.
- `scripts/assets/` for asset generation helpers.
- `scripts/debug/` for troubleshooting launchers.

## Branding Assets

- `assets/branding/source/` for source art.
- `assets/branding/generated/` for generated exports, including installer backgrounds.

## Archived Docs

- `docs/archive/` for historical plans and implementation summaries.

## Generated Output

- `dist/`
- `src-tauri/target/`
- `runtime/asr/`
- `node_modules/`
