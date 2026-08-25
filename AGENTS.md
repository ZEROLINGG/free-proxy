# AGENTS.md

Guidance for AI coding agents working in this repo. User-facing docs, code comments, and UI strings are written in Chinese — match that.

## Directory Tree

> `gen/`、`icons/`、`image/`、`logs/`、各 `Cargo.lock` 为构建产物、资源或运行时文件，通常无需修改。

```
./
├── AGENTS.md
├── client_cli/
│   ├── Cargo.lock
│   ├── Cargo.toml
│   └── src/
│       ├── ca.rs
│       ├── config.rs
│       ├── health.rs
│       ├── main.rs
│       ├── run.rs
│       ├── speed.rs
│       └── subscribe.rs
├── client_tauri/
│   ├── index.html
│   ├── package.json
│   ├── pnpm-lock.yaml
│   ├── postcss.config.js
│   ├── public/
│   ├── README.md
│   ├── src/
│   │   ├── App.tsx
│   │   ├── assets/
│   │   ├── components/
│   │   │   ├── layout/
│   │   │   │   ├── BottomTabs.tsx
│   │   │   │   ├── GlassTopbar.tsx
│   │   │   │   ├── Layout.tsx
│   │   │   │   └── Sidebar.tsx
│   │   │   └── ui/
│   │   │       ├── Badge.tsx
│   │   │       ├── BottomSheet.tsx
│   │   │       ├── Button.tsx
│   │   │       ├── ColoredCta.tsx
│   │   │       ├── Input.tsx
│   │   │       ├── Panel.tsx
│   │   │       ├── Progress.tsx
│   │   │       ├── Segmented.tsx
│   │   │       ├── Select.tsx
│   │   │       ├── Spinner.tsx
│   │   │       ├── Switch.tsx
│   │   │       └── Toast.tsx
│   │   ├── lib/
│   │   │   ├── tauri.ts
│   │   │   └── types.ts
│   │   ├── main.tsx
│   │   ├── pages/
│   │   │   ├── About.tsx
│   │   │   ├── CaCert.tsx
│   │   │   ├── Dashboard.tsx
│   │   │   ├── ProxySettings.tsx
│   │   │   └── SpeedTest.tsx
│   │   ├── store/
│   │   │   ├── proxy.ts
│   │   │   ├── settings.ts
│   │   │   ├── speedTest.ts
│   │   │   └── ui.ts
│   │   ├── styles/
│   │   │   └── globals.css
│   │   └── vite-env.d.ts
│   ├── src-tauri/
│   │   ├── build.rs
│   │   ├── capabilities/
│   │   │   └── default.json
│   │   ├── Cargo.lock
│   │   ├── Cargo.toml
│   │   ├── gen/
│   │   │   ├── android/
│   │   │   │   ├── app/
│   │   │   │   ├── build.gradle.kts
│   │   │   │   ├── buildSrc/
│   │   │   │   ├── free-proxy.jks
│   │   │   │   ├── gradle/
│   │   │   │   ├── gradle.properties
│   │   │   │   ├── gradlew*
│   │   │   │   ├── gradlew.bat
│   │   │   │   ├── keystore.properties
│   │   │   │   ├── settings.gradle
│   │   │   │   └── tauri.settings.gradle
│   │   │   └── schemas/
│   │   │       ├── acl-manifests.json
│   │   │       ├── android-schema.json
│   │   │       ├── capabilities.json
│   │   │       ├── desktop-schema.json
│   │   │       ├── linux-schema.json
│   │   │       └── mobile-schema.json
│   │   ├── icons/
│   │   │   ├── 128x128@2x.png
│   │   │   ├── 128x128.png
│   │   │   ├── 32x32.png
│   │   │   ├── 64x64.png
│   │   │   ├── android/
│   │   │   ├── free-proxy-on.svg
│   │   │   ├── free-proxy.svg
│   │   │   ├── icon.icns
│   │   │   ├── icon.ico
│   │   │   ├── icon.png
│   │   │   ├── ios/
│   │   │   ├── Square*.png
│   │   │   └── StoreLogo.png
│   │   ├── src/
│   │   │   ├── commands/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── proxy.rs
│   │   │   │   ├── settings.rs
│   │   │   │   └── speed.rs
│   │   │   ├── lib.rs
│   │   │   ├── main.rs
│   │   │   └── tray.rs
│   │   └── tauri.conf.json
│   ├── tsconfig.json
│   ├── tsconfig.node.json
│   └── vite.config.ts
├── deny.toml
├── image/
├── lib/
│   ├── Cargo.lock
│   ├── Cargo.toml
│   └── src/
│       ├── aead.rs
│       ├── algo.rs
│       ├── base.rs
│       ├── compress.rs
│       ├── ecc.rs
│       ├── frames.rs
│       ├── hash.rs
│       ├── http.rs
│       ├── kdf.rs
│       ├── lib.rs
│       ├── proxy/
│       │   ├── body.rs
│       │   ├── client.rs
│       │   ├── connection.rs
│       │   ├── mod.rs
│       │   ├── relay.rs
│       │   ├── tls.rs
│       │   └── ws.rs
│       ├── speed_test/
│       │   ├── health.rs
│       │   ├── ip.rs
│       │   ├── mod.rs
│       │   └── tcping.rs
│       ├── tool.rs
│       └── ws.rs
├── lib_test/
│   ├── .gitignore
│   ├── Cargo.lock
│   ├── Cargo.toml
│   └── src/
│       ├── cs.rs
│       ├── main.rs
│       ├── test/
│       │   ├── base.rs
│       │   ├── http.rs
│       │   └── mod.rs
│       └── web.rs
├── logs/
├── package.json
├── README.md
└── server-rs/
    ├── Cargo.lock
    ├── Cargo.toml
    ├── src/
    │   ├── app.rs
    │   ├── lib.rs
    │   ├── proxy_http.rs
    │   ├── proxy_ws.rs
    │   └── subscribe.rs
    └── wrangler.toml
```

## Build model (non-obvious)

- Five **standalone crates, no root Cargo workspace**: `lib/`, `client_cli/`, `client_tauri/src-tauri/`, `server-rs/`, `lib_test/` (each has its own `target/`). Cargo commands must be run inside a crate directory — from the repo root they fail with "could not find Cargo.toml". In particular, README's `cargo test -p lib` only works if run from inside `lib/`.
- `lib` is shared source between native clients and the Cloudflare Worker:
  - default `client` feature pulls in tokio/reqwest/rustls (MITM TLS) — used by both clients;
  - `server-rs` depends on it with `default-features = false` (no tokio), compiled to wasm32 by `worker-build`.
  - After changing `lib`, verify **both** configurations (see commands below) — native-only checks won't catch wasm-side breakage.
- `server-rs` is a Cloudflare Worker (Rust → wasm32, `worker` crate 0.8 + axum). Real builds/deploy go through wrangler (`worker-build`); plain `cargo check` inside `server-rs/` still works for fast native typechecks.
- `lib_test` is a **binary E2E harness**, not `#[test]` code — run it with `cargo run`, not `cargo test`. It orchestrates the full chain itself (`src/cs.rs`): spawns the local Worker via `pnpm server-dev` (needs a free port 80 + pnpm on PATH), writes a **random key into `server-rs/.dev.vars`** (overwrites your dev secrets), starts the proxy client (fixed Aes128Gcm + Lz4, port 18081, CA in temp dir) and a local axum target site (port 18082), then drives real requests through the tunnel (`src/test/base.rs`: example.com HTTP/HTTPS + localhost) with a custom colored-report runner (`src/test/mod.rs`, `test_fn!` macro). Requires internet access; not wired into CI.

## Commands

From repo root (see `package.json`):

- `npm run server-dev` — local Worker via wrangler; binds **port 80**, reads secrets from `server-rs/.dev.vars` (gitignored: `key`, `domain`)
- `npm run server-deploy` — deploy Worker to Cloudflare
- `npm run client-dev` / `npm run client-android-dev` — Tauri desktop / Android dev
- `npm run test-e2e` — run the `lib_test` E2E harness (same as `cd lib_test && cargo run`; see caveats above)

Per-crate:

- Tests: `cd lib && cargo test` — ~110 tests incl. client↔server contract/roundtrip tests, offline, ~20 s. Two ignored tests (`speed_test::tcping`, `speed_test::health`) need network access / a locally running worker.
- E2E harness: `cd lib_test && cargo run` — self-hosted full-chain test (Worker on 80, proxy client on 18081, target site on 18082); needs pnpm + free port 80 + internet, and **rewrites `server-rs/.dev.vars`** with a random key (restore your own dev secrets afterwards if needed).
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
