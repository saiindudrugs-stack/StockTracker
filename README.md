# Portfolio Management Application — Volume II, Slice 1 + Desktop Shell

Portfolio Engine + Zerodha Adapter + Live Feed Manager, per the Volume I HLD/SRS
(Sections 2, 3, 5) and the decisions locked in through v1.4 of that document —
now wired into a real Tauri desktop shell with a minimal React UI.

## How to run this

**Just the engine (no UI, works anywhere, including this sandbox):**
```bash
cargo test -p pm-domain -p pm-application -p pm-infrastructure
```
34 tests, zero warnings, no external dependencies (credentials, network,
GUI) needed.

**The actual desktop app, on your own machine:**
```bash
cd apps/desktop
npm install
npm run tauri dev      # opens a window with a live-reloading dashboard
```
This needs Rust (stable, 1.77+) and Node 20+ installed locally — see
"Why this sandbox can't run the app" below for why that matters.

**Installers (.dmg for Mac, .msi for Windows) without owning a Mac or PC:**
push this to a GitHub repo and either run the "Build desktop installers"
workflow manually from the Actions tab, or push a `v0.1.0`-style tag to get
a draft GitHub Release with all three installers attached. See
`.github/workflows/build.yml` — full instructions below in "Getting
installers via GitHub Actions."

## Why this sandbox can't run the app (but can run everything else)

The engine crates (`domain`, `application`, `infrastructure`) compile and
test fully in this sandbox and are genuinely verified — 34 passing tests,
zero warnings. The desktop shell (`apps/desktop/src-tauri`) is a different
story: Tauri's Linux/Windows/macOS windowing stack (`wry`, `tao`) has raised
its minimum supported Rust version a few times over the last year, and
currently needs **Rust 1.77+**. This sandbox's Cargo is version 1.75 (from
an older Ubuntu package, with no `rustup` and no route to a newer toolchain —
static.rust-lang.org isn't reachable from here). That's a limitation of
*this specific verification environment*, not of the code: on your machine,
or in GitHub Actions (which always runs current stable Rust), this builds
normally with the standard version ranges in `Cargo.toml` — no exotic
pinning needed for the desktop app itself (unlike the engine crates, which
*do* need a few pins to resolve at all under Cargo 1.75 — see the note near
the bottom of this file).

The frontend (React/Vite/TypeScript in `apps/desktop/src`) doesn't have this
problem — it's already been built and verified in this sandbox with
`npm run build`.

## Getting installers via GitHub Actions

1. Push this whole directory to a new GitHub repo (`git init`, `git add -A`,
   `git commit`, create a repo on GitHub, `git push`).
2. Go to the repo's **Actions** tab. You'll see "Build desktop installers."
3. Either:
   - Click **Run workflow** (manual trigger) — builds all three platforms,
     installers show up as downloadable **Artifacts** on that run's page
     (bottom of the summary), or
   - Push a tag like `git tag v0.1.0 && git push origin v0.1.0` — same
     three builds, plus a **draft Release** with the `.dmg`, `.msi`, and
     `.deb`/`.AppImage` attached, ready to publish when you're happy with it.
4. Each platform builds on its own real OS (macOS runner produces the
   `.dmg`, Windows runner produces the `.msi`, Linux runner produces
   `.deb`/`.AppImage`) — this is the standard way cross-platform Tauri apps
   ship; nobody builds a `.dmg` by cross-compiling from Linux.

The installers are **unsigned** (no Apple Developer certificate or
Microsoft code-signing cert configured). macOS will show a Gatekeeper
warning ("unidentified developer") the first time you open it — right-click
→ Open bypasses that for local testing. Signing is worth setting up before
sharing this with anyone else; it's an Apple Developer Program
enrollment + a few GitHub Actions secrets, not a code change, so it's a
separate task from what's built here.

## Architecture (matches HLD Section 3.1)

```
apps/desktop/          — Tauri shell + React UI (thin — no business logic)
  src-tauri/                Tauri commands: deserialize IPC args, call a
                             use-case, serialize the result. Seeds two demo
                             instruments (RELIANCE, TCS) on first launch so
                             the dashboard has something to show.
  src/App.tsx               Minimal UI: net worth/P/L cards, a holdings
                             table, a "record a buy" form, per-symbol XIRR —
                             proves the IPC round-trip works, not the full
                             dashboard from the Volume I wireframes yet.

crates/
  domain/          — pure business logic, zero I/O dependencies
    value_objects.rs   Money (rust_decimal, never f64), Isin
    entities.rs         Instrument, Transaction, Holding (derived, never
                         hand-edited — Holding::apply is the only mutator),
                         Portfolio
    analytics/xirr.rs    Newton-Raphson XIRR solver with bisection fallback
    repositories.rs      Repository TRAITS only — no SQLite/DuckDB here

  application/     — use-cases orchestrating domain + repository traits
    use_cases/record_transaction.rs   validates the FULL ledger fold before
                                       persisting anything (see "bug fixed"
                                       below)
    use_cases/rebuild_holdings.rs     bulk rebuild after import/backfill
    use_cases/compute_xirr.rs         cashflow construction + mark-to-market
    use_cases/dashboard_summary.rs    net worth / P/L aggregation

  infrastructure/  — implements the domain's repository traits
    sqlite/              transactional store (SQLite, NOT YET SQLCipher —
                          see "known simplifications")
    brokers/zerodha.rs    real Kite Connect request/response shapes,
                          checksum auth — NOT integration-tested (no live
                          Kite account available in this environment)
    live_feed/            WebSocket transport + exponential backoff +
                          1-minute bar aggregator (confirmed resolution,
                          Volume I Section 11)
```

Dependency direction is strictly inward: `infrastructure` depends on `domain`,
`application` depends on `domain`, `apps/desktop/src-tauri` depends on all
three, but `domain` depends on nothing. This is what the HLD means by
"swappable" — SQLite → DuckDB, Zerodha → Upstox, all without touching a
use-case, an entity, or the UI.

## A real bug I found and fixed while building this

The first draft of `RecordTransactionUseCase` recorded the transaction to the
ledger **before** validating it against the full holding history. That means
an invalid sell (overdrawing a position) would have been written into the
append-only ledger — exactly the kind of entry the domain layer's own
"Auditability" comment says shouldn't need a manual correction. Fixed to
validate the full fold first, and persist only once that succeeds. Covered by
`invalid_sell_returns_error_and_is_not_left_in_a_bad_snapshot`, which asserts
zero rows in the ledger after a rejected transaction, not just that an error
came back.

## Known simplifications (flagged, not hidden)

1. **SQLite, not SQLCipher.** The transactional store isn't encrypted at rest
   yet. Swapping in encryption is a matter of the `bundled-sqlcipher` rusqlite
   feature and threading a key through `SqlitePool::open`, and touches only
   `sqlite/mod.rs` — nothing in `domain` or `application` changes.
2. **price_history/intraday_bar are SQLite-backed, not DuckDB.** The HLD
   (Section 5.2) calls for DuckDB once real intraday volume is flowing. The
   `PriceRepository` trait is exactly the seam for that swap — same story as
   #1, isolated to one file (`sqlite/price_repository.rs`).
3. **Zerodha's actual WebSocket tick format isn't implemented.** Kite ticks
   arrive as a compact binary packet (documented in Kite Connect's API docs,
   not something to reverse-engineer from memory without a live account to
   verify against). `TickDecoder` is the trait seam for it — `manager.rs`'s
   tests use a fake decoder to prove the reconnect/aggregation logic is
   correct independent of that byte format.
4. **`AuthCredentials` doesn't fit Kite's three-part handshake.** Kite needs
   api_key + request_token + api_secret; the broker-agnostic trait as
   written only carries two. `zerodha.rs`'s `authenticate()` reads the secret
   from an env var as a stopgap and says so in a comment — worth revisiting
   in the next LLD pass before adding a second broker.
5. **Historical daily-candle fetch from Zerodha isn't implemented** — it's
   keyed by an internal `instrument_token`, not ISIN, requiring the
   instruments master-data dump to resolve. Returns an explicit error rather
   than a guessed mapping.

None of these block the next slice (Tauri wiring + dashboard) — they're all
behind trait boundaries that don't leak into the code that would call them.

## Running it

```bash
cd pmapp-code
cargo test --workspace     # 34 tests, ~1 min including dependency compile
cargo build --workspace    # zero warnings
```

No credentials or network access needed to run the test suite — the Zerodha
adapter tests cover checksum generation and JSON parsing in isolation; the
Live Feed Manager tests use a fake transport, not a real socket.

## A note on the build environment

Getting `cargo generate-lockfile` to succeed at all took real work: this
sandbox runs Cargo 1.75 (Dec 2023), and current crates.io has packages
requiring the `edition2024` feature Cargo 1.75 doesn't understand — even
merely *resolving* dependencies (not building them) fails if any candidate
version in the graph needs it. `rust_decimal`'s optional `borsh` feature was
one culprit (pulled in `proc-macro-crate` → `toml_edit` → `toml_datetime`),
as was `uuid`'s default `getrandom` version, and `reqwest`'s `h2`/`idna`
chain. All are pinned to older, compatible versions in `Cargo.toml` with
comments at each pin. On a modern toolchain (1.85+, matching what a real dev
machine or CI would run) these pins are almost certainly unnecessary and can
be relaxed — I'd suggest trying that first when you move this off the
sandbox, rather than assuming the pins are permanent.

## Next slice

The desktop shell now exists (this update) with a minimal but real UI. What's
still ahead: replacing the demo-data UI with the actual dashboard from the
Volume I wireframes, wiring the Zerodha adapter's `authenticate()`/
`fetch_holdings()` into a real "Connect broker" flow in the UI (currently
only reachable from Rust, not exposed as a Tauri command yet), finishing the
Zerodha tick decoder against real Kite documentation once there's an account
to verify the byte format against, and code-signing the installers before
sharing them with anyone beyond yourself.
