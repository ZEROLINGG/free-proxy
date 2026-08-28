# AGENTS.md

Guidance for AI coding agents working in this repo. User-facing docs, code comments, and UI strings are written in Chinese — match that.

## Layout

Five **standalone Rust crates, no root Cargo workspace** — Cargo commands fail from the repo root with "could not find Cargo.toml"; always run them inside a crate dir (each has its own `target/`):

- `lib/` — shared protocol core, compiled by BOTH native clients and the Cloudflare Worker
- `client_cli/` — CLI client
- `client_tauri/` — GUI client: React 19 + Vite frontend (`src/`), Tauri 2 backend (`src-tauri/`)
- `server-rs/` — Cloudflare Worker (Rust → wasm32 via `worker-build`; worker crate 0.8 + axum)
- `lib_test/` — E2E harness **binary** (`cargo run`, never `cargo test`)

Leave alone: `client_tauri/src-tauri/gen/`, `icons/`, `image/`, `logs/`, per-crate `Cargo.lock`.

CLI and GUI deliberately share one config/CA directory (`app_data_dir/com.zz.freeproxy`) — the identifier is hardcoded in both `client_cli/src/config.rs` (`IDENTIFIER`) and the Tauri config; changing either desyncs the two clients.

## Directory structure

Annotated map of source dirs (build/runtime dirs like `target/`, `.wrangler/`, `icons/`, `image/`, `logs/` are omitted):

```
├── package.json                  # root npm scripts (server-dev/deploy, client-dev, test-e2e)
├── deny.toml                     # cargo-deny shared config (all 4 crates read it)
├── .github/workflows/release.yml # tag-triggered release CI (desktop + CLI + Worker zip; Android job commented out)
├── lib/src/                      # shared protocol core — compiles for native AND wasm32
│   ├── aead.rs                   #   AEAD ciphers: AES-GCM/GCM-SIV, ChaCha20-Poly1305/XChaCha20
│   ├── algo.rs                   #   compressor × AEAD negotiation + URL contract /api/{version}/{target}
│   ├── base.rs                   #   base encodings (base64 / z85 / base91)
│   ├── compress.rs               #   zstd / gzip / lz4
│   ├── ecc.rs                    #   ECDSA/ECDH (k256/p256/p384)
│   ├── frames.rs                 #   binary frame stream: [u32 BE len | payload], zero-length frame = EOS
│   ├── hash.rs                   #   sha1/sha2/blake3 wrappers
│   ├── http.rs                   #   httparse header parsing + UrlBuilder (zero-copy)
│   ├── kdf.rs                    #   PBKDF2 / scrypt / HKDF wrappers
│   ├── tool.rs                   #   derive_keys (auth_key+domain), time-window token auth, XOR obfuscation
│   ├── ws.rs                     #   RFC 6455 frames + WsTunnelMsg (shared by client & worker)
│   ├── proxy/                    #   CLIENT ONLY (feature "client"): local HTTP proxy
│   │   ├── mod.rs                #     ProxyConfig / Shared / Proxy lifecycle facade
│   │   ├── connection.rs         #     connection dispatch (plain HTTP vs CONNECT)
│   │   ├── relay.rs              #     forwarding engine (serve loop, keep-alive via EOS)
│   │   ├── body.rs               #     request-body boundary parsing
│   │   ├── client.rs             #     upstream reqwest clients (main/pref-IP/WS HTTP1.1)
│   │   ├── tls.rs                #     MITM TLS: self-signed CA + per-SNI leaf certs (moka cache)
│   │   └── ws.rs                 #     WS tunnel client side (RFC6455 parse/mask/reassemble)
│   └── speed_test/               #   CLIENT ONLY: 优选 IP two-phase speed test
│       ├── tcping.rs health.rs   #     phase 1 tcping → phase 2 Worker /health check
│       └── ip.rs                 #     Cloudflare IP candidate ranges
├── server-rs/
│   ├── wrangler.toml             # worker config; [dev] port=80; DO NOT touch compatibility_flags
│   ├── .dev.vars                 # gitignored dev secrets (key/domain); E2E harness rewrites it!
│   └── src/
│       ├── app.rs                #   axum router, Bearer auth middleware (±30 s window), routes
│       ├── proxy_http.rs         #   POST /api/{version}/{target}: streaming decrypt→fetch→re-encrypt
│       ├── proxy_ws.rs           #   GET /ws/{version}/{target}: upstream WS handshake + full-duplex relay
│       ├── subscribe.rs          #   GET /subscribe/{port}: Clash / sing-box / base64 subscription
│       └── lib.rs                #   worker entrypoint
├── client_cli/src/               # CLI client: main.rs (clap), run.rs (proxy loop), speed.rs,
│                                 # health.rs, ca.rs (cert install), subscribe.rs,
│                                 # config.rs (settings.json — shares app_data_dir with GUI)
├── lib_test/                     # E2E harness binary (`cargo run`, never `cargo test`)
│   ├── Cargo.lock
│   ├── Cargo.toml
│   └── src/
│       ├── cs.rs                 #   full-chain orchestrator (Worker + proxy + target site)
│       ├── main.rs               #   harness entrypoint
│       ├── test/
│       │   ├── base.rs           #     basic reachability cases (example.com http/https)
│       │   ├── http.rs           #     HTTP proxy test cases
│       │   └── mod.rs            #     shared proxied client + colored report runner
│       ├── util.rs               #   helpers
│       └── web.rs                #   local target site
└── client_tauri/
    ├── src/                      # React 19 frontend: pages/ (Dashboard, ProxySettings,
    │                             # SpeedTest, CaCert, About), components/{layout,ui}/,
    │                             # store/, lib/, styles/globals.css
    └── src-tauri/
        ├── tauri.conf.json       # release version source of truth (CI checks tag against it)
        ├── capabilities/default.json
        ├── gen/android/          # generated Android project (+ local keystore files)
        ├── gen/schemas/          # generated Tauri capability/ACL JSON schemas (regenerated, don't edit)
        └── src/
            ├── commands/         # Tauri commands: proxy.rs, speed.rs, settings.rs
            ├── tray.rs           # system tray
            └── lib.rs main.rs    # app setup / entrypoint
```

## Build model

- `lib` compiles in two modes:
  - default `client` feature → tokio/reqwest/rustls MITM TLS stack, used by both clients;
  - `server-rs` depends on it with `default-features = false` (no tokio) and compiles to wasm32.
  - After changing `lib`, check **both** configurations (commands below) — native-only checks won't catch wasm-side breakage.
- `server-rs`: plain `cargo check` inside its dir works for fast native typechecks; real builds/deploy go through wrangler (`worker-build`).

## Commands

From repo root (see `package.json`):

- `npm run server-dev` — local Worker via wrangler; binds **port 80**, reads secrets from `server-rs/.dev.vars` (gitignored: `key`, `domain`)
- `npm run server-deploy` — deploy Worker to Cloudflare
- `npm run client-dev` / `npm run client-android-dev` — Tauri desktop / Android dev
- `npm run test-e2e` — run the `lib_test` E2E harness

Per-crate:

- Unit tests: from inside `lib/`, `cargo test` (~110 offline tests incl. client↔server contract/roundtrip tests, ~20 s). README's `cargo test -p lib` also only works from inside `lib/`. 
- E2E harness: `cd lib_test && cargo run` — self-hosted full-chain test (Worker on 80, proxy client on 18081, target site on 18082); needs pnpm + free port 80 + internet, and **rewrites `server-rs/.dev.vars` with a random key** (restore your own dev secrets afterwards if needed). Runs all 46 cases; exits non-zero unless every function classifies as 稳定成功/不稳定成功.
- Wasm-side check of shared lib: `cd lib && cargo check --no-default-features --features server`
- Security audit (run per crate; shared config at repo-root `deny.toml`): `cargo deny check licenses advisories` + `cargo audit`. Advisory ignores live in `deny.toml [advisories]` with justifications — review them when bumping `postcard`, `tauri`, or the gtk/wry stack.
- Frontend typecheck+build: `pnpm install && pnpm build` inside `client_tauri/` (tsc + vite). Toolchain: Node 22, pnpm 10 (matches CI).
- Unified logging (`lib/src/log.rs`): use `crate::error!/warn!/info!/debug!/trace!` (or `lib::*` from outside crates) instead of `eprintln!`/`console_error!`. Native entrypoints call `lib::log::init` (CLI: stderr + ANSI; GUI: `app_data_dir/logs/freeproxy.log`, ANSI off); the Worker lazily calls `lib::log::init_wasm` from `fetch`. Level/paths filter via `RUST_LOG` (e.g. `RUST_LOG=debug`). `lib::log::LogConfig` and native `init` are `#![cfg(feature = "client")]` — server crate compiles them out.

## CI

- Pushing to `main` triggers `.github/workflows/test.yml` — the E2E gate runs the full `lib_test` suite and fails the commit if any function isn't StableSuccess/UnstableSuccess.

## Release flow

- Releasing = pushing a `v*` tag; CI (`.github/workflows/release.yml`) builds Tauri desktop (Win/macOS x2/Linux), CLI, and Worker zip.
- CI hard-fails unless the tag equals `"version"` in `client_tauri/src-tauri/tauri.conf.json` — **that file is the release version source of truth, not Cargo.toml** (crate versions drift from it).
- The CI Android job is commented out — Android builds happen locally only, even though `gen/android/` exists.

## Fragile spots

- `server-rs/wrangler.toml` sets `compatibility_flags = ["no_websocket_standard_binary_type"]`. Do not remove or "upgrade" this: worker crate 0.8 requires ArrayBuffer WS messages, and the modern default (compat date ≥ 2026-03-17) delivers Blobs, which silently empties WS tunnel frames. See comment in that file and upstream fix https://github.com/cloudflare/workers-rs/pull/1049 .
- Secrets (`key`, `domain`) live in Cloudflare Worker secrets / `.dev.vars`. Never commit `.dev.vars*`, `*.pem`, `*.key`, etc. (all gitignored).

## Documentation Sync (README.md)

**MANDATORY RULE:** The root `README.md` is the face of the project. You MUST proactively review and update it in the same context if your changes trigger any of the following conditions:

1. **UX & Feature Changes:**
  - Modifying CLI arguments (`client_cli/src/main.rs` or `clap` structs).
  - Adding/altering GUI pages or workflows in Tauri (`client_tauri/src`).
  - Introducing new configuration fields in `settings.json` or changing the config logic.
  - Changing API endpoints, subscription logic, or supported proxy protocols.
2. **Setup & Architecture Changes:**
  - Modifying npm/Cargo commands in `package.json` or `Makefile`.
  - Changing default ports (e.g., Worker port 80, local proxy port 18081).
  - Adding a new directory or crate.
3. **Large-scale Refactoring:** Any change touching >5 files or altering the core data flow.

**Agent Self-Check Prompt:** Before finishing a task, ask yourself: *"Does this code change make the current `README.md` obsolete, inaccurate, or missing new instructions?"* If yes, update `README.md` immediately. **Remember: Keep the README updates in Chinese.**