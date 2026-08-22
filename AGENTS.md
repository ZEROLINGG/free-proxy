# AGENTS.md

Guidance for AI coding agents working in this repo. User-facing docs, code comments, and UI strings are written in Chinese — match that.

## Build model (non-obvious)

- Four **standalone crates, no root Cargo workspace**: `lib/`, `client_cli/`, `client_tauri/src-tauri/`, `server-rs/` (each has its own `target/`). Cargo commands must be run inside a crate directory — from the repo root they fail with "could not find Cargo.toml". In particular, README's `cargo test -p lib` only works if run from inside `lib/`.
- `lib` is shared source between native clients and the Cloudflare Worker:
  - default `client` feature pulls in tokio/reqwest/rustls (MITM TLS) — used by both clients;
  - `server-rs` depends on it with `default-features = false` (no tokio), compiled to wasm32 by `worker-build`.
  - After changing `lib`, verify **both** configurations (see commands below) — native-only checks won't catch wasm-side breakage.
- `server-rs` is a Cloudflare Worker (Rust → wasm32, `worker` crate 0.8 + axum). Real builds/deploy go through wrangler (`worker-build`); plain `cargo check` inside `server-rs/` still works for fast native typechecks.

## Commands

From repo root (see `package.json`):

- `npm run server-dev` — local Worker via wrangler; binds **port 80**, reads secrets from `server-rs/.dev.vars` (gitignored: `key`, `domain`)
- `npm run server-deploy` — deploy Worker to Cloudflare
- `npm run client-dev` / `npm run client-android-dev` — Tauri desktop / Android dev

Per-crate:

- Tests: `cd lib && cargo test` — ~110 tests incl. client↔server contract/roundtrip tests, offline, ~20 s. Two ignored tests (`speed_test::tcping`, `speed_test::health`) need network access / a locally running worker.
- Wasm-side check of shared lib: `cd lib && cargo check --no-default-features --features server`
- Security audit (per crate, shared config at repo-root `deny.toml`): `cargo deny check licenses advisories` + `cargo audit`. Advisory ignores live in `deny.toml [advisories]` with justifications — review them when bumping `postcard`, `tauri`, or the gtk/wry stack.
- Frontend typecheck+build: `pnpm install && pnpm build` inside `client_tauri/` (tsc + vite). Toolchain: Node 22, pnpm 10 (matches CI).

## Release flow

- Releasing = pushing a `v*` tag; CI (`.github/workflows/release.yml`) builds Tauri desktop, CLI, and Worker zip.
- CI hard-fails unless the tag equals `"version"` in `client_tauri/src-tauri/tauri.conf.json` — **that file is the release version source of truth, not Cargo.toml** (crate versions drift from it).

## Fragile spots

- `server-rs/wrangler.toml` sets `compatibility_flags = ["no_websocket_standard_binary_type"]`. Do not remove or "upgrade" this: worker crate 0.8 requires ArrayBuffer WS messages, and the modern default delivers Blobs, which silently empties WS tunnel frames.
- Secrets (`key`, `domain`) live in Cloudflare Worker secrets / `.dev.vars`. Never commit `.dev.vars`, `*.pem`, `*.key`, etc. (all gitignored).

Architecture details and request-flow diagrams live in `README.md` (目录结构 / 工作原理 sections) — consult before touching `lib` protocol modules (`frames.rs`, `algo.rs`, `ws.rs`).
