# AGENTS.md

## Cursor Cloud specific instructions

This is a **Tauri v2 desktop app** (Rust backend + React/TypeScript frontend + embedded SQLite). No external databases, Docker, or network services are needed.

### Key commands

All standard dev commands are in `package.json`:

| Task | Command |
|---|---|
| Install JS deps | `npm install` |
| Frontend dev server only | `npm run dev` (Vite on port 1420) |
| Rust core tests (27 unit tests) | `npm run test:rust` |
| TypeScript type-check + Vite build + Rust tests | `npm run check` |
| Rust format check | `cargo fmt --check` |
| Launch full desktop app (compiles Rust + starts Vite) | `npm run tauri:dev` |

### Gotchas

- **First `npm run tauri:dev` compiles ~440 Rust crates** (~70s). Subsequent runs are incremental (~2-5s).
- The Tauri config (`src-tauri/tauri.conf.json`) runs `npm run dev` as `beforeDevCommand`, so `npm run tauri:dev` starts the Vite dev server automatically — do not start Vite separately.
- **Linux system deps** (WebKitGTK, GTK3, etc.) are required for Tauri and are pre-installed by the VM update script. If they are missing, install with: `sudo apt-get install -y libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libsoup-3.0-dev libjavascriptcoregtk-4.1-dev`.
- `libEGL` warnings at app launch are expected in headless/VM environments without GPU — they do not affect functionality.
- Rust toolchain is pinned to **1.88.0** via `rust-toolchain.toml`; rustup handles this automatically.
- There is **no ESLint or JS-level linter** configured; lint checking is TypeScript (`tsc`) + `cargo fmt`.
- There are **no frontend tests** (no vitest/jest); only Rust unit tests exist in `crates/music-os-core/`.
