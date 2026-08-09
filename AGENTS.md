# BITIG Engineering Guide

## Purpose

BITIG is a local-first Tauri 2 desktop media downloader and managed media library. It is intended only for media the user owns or is allowed to download; do not add DRM, paywall, or authentication bypasses.

## Architecture

- React/TypeScript owns presentation and user interaction.
- Tauri commands are thin adapters into Rust services.
- Rust services own validation, subprocesses, download state, files, and SQLite transactions.
- Repositories are the only layer that issues SQLite queries.
- Managed media paths are resolved from Tauri AppLocalData; never trust a physical path supplied by the frontend.

Important locations: `src/` (frontend), `src-tauri/src/` (Rust), `src-tauri/migrations/` (ordered SQL migrations), `src-tauri/binaries/` (platform sidecars), and `docs/` (deeper design notes).

## Commands

- Development: `npm run tauri dev`
- Frontend: `npm run typecheck`, `npm run lint`, `npm run test`, `npm run build`
- Rust: `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check`
- Rust lint/tests: `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`, `cargo test --manifest-path src-tauri/Cargo.toml --all-targets`

## Database changes

Add a new forward-only numbered SQL file under `src-tauri/migrations/`. Never edit a migration that may already have been applied. Migrations run automatically during application bootstrap.

## Safety rules

- Pass every subprocess argument separately through `tokio::process::Command`; never build a shell string.
- Write downloads to `temp/<job-id>/`, verify them with ffprobe, then atomically finalize them into `media/`.
- Keep physical filenames as internal IDs. Rename and move operations update SQLite metadata only.
- Keep Tauri capabilities narrow; the frontend must not receive arbitrary shell, filesystem, or SQLite access.
- Never log cookies, tokens, secret headers, or full sensitive command output.
- Do not weaken lint/type/security checks or add unpinned dependencies to make a build pass.

See `ARCHITECTURE.md` and `docs/development/sidecars.md` for details.
