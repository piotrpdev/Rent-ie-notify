# AGENTS.md

Single-binary Rust app (edition 2024): fetches rent.ie and/or daft.ie search results from their JSON APIs, dedupes against `sent_ids.txt`, and posts new ones to a Slack webhook. Runs hourly via systemd; the GitHub Actions workflow is manual-only right now.

## Layout

- `src/main.rs` — orchestration: env vars, per-source fetching, dedupe, Slack POST, state file.
- `src/listing.rs` — shared `Listing` model plus description/date formatting.
- `src/rent_ie.rs` — builds the rent.ie `_next/data` search API URL from `__NEXT_DATA__.page`/`query` and parses the API JSON; `tl` locates the metadata script, `nanoserde` parses JSON.
- `src/daft_ie.rs` — parses daft.ie gateway API responses; links use `https://www.daft.ie<seoFriendlyPath>`.
- `src/slack.rs` — Slack Block Kit payload built with `nanoserde`.
- `archives/rentie/` / `archives/daftie/` — saved API JSON responses for local verification; not read at runtime.
- `check_rent_ie.service` / `check_rent_ie.timer` — hourly systemd deployment; hardcodes user, repo/binary paths, and env vars.
- `.github/workflows/hourly_listings.yaml` — builds/runs with `cargo run --locked --release`; persists `sent_ids.txt` as a workflow artifact; `schedule` is commented out.

## Commands

- Check: `cargo fmt --check && cargo clippy --locked --all-targets -- -D warnings && cargo test --locked` (9 unit tests, plus 2 ignored archive-backed tests).
- Archive check: `cargo test --locked -- --ignored` (parses the rent.ie and daft.ie fixtures in `archives/`).
- Build: `cargo build --release` → `target/release/Rent-ie-notify`.
- Run from the repo root with `SLACK_WEBHOOK_URL` and at least one of `RENT_IE_URL` or `DAFT_IE_REQUEST` set.
- CI pins Rust 1.92.0 (workflow); locally installed toolchains may be newer, so avoid APIs newer than that.

## Gotchas

- `sent_ids.txt` is gitignored runtime state, read/written relative to the current directory. Running elsewhere moves the state. A missing file treats every fetched listing as new; deleting it re-sends everything.
- There is no dry-run or argument parsing: any `cargo run` with valid env vars hits the configured APIs, Slack, and appends IDs. Use a throwaway webhook for end-to-end checks.
- Slack's HTTP status is not checked; a transport-level success is enough to mark listings as sent, so a rejected payload can be lost silently. One message is sent per source, and empty sources are skipped.
- rent.ie listing data comes from `/_next/data/<buildId>/search/<rest>.json?...`. The `buildId` changes on deployments and isn't available from a stable endpoint, so the configured search page is still fetched once per run to read it from `<script id="__NEXT_DATA__">`.
- `rent_ie::search_api_url` derives the API route and query parameters from `__NEXT_DATA__.page`/`query`; don't hand-build it from the public URL, which omits the county/area split. Filters such as max rent (`rent_0-650`) are additional `rest` path segments, not special-cased.
- rent.ie IDs come from the numeric `id` field in the API response. Legacy `sent_ids.txt` entries in the old `<slug>/<id>` format are normalized to the final path segment when loaded, so they keep matching.
- rent.ie `seoFriendlyPath` is a `/share/<slug>/<id>` path that 404s. Links are constructed as `https://www.rent.ie/<pageProps.propertyType>/<slug>/<id>/`; the section varies (`rooms-to-rent`, `houses-to-let`, etc.).
- daft.ie uses a POST to `https://gateway.daft.ie/api/v2/ads/listings`. `DAFT_IE_REQUEST` is the JSON request body; it needs `accept`/`content-type`, `brand: daft`, `platform: web`, `origin`, and `referer` (set in `main.rs`). Browser-only `sec-*`/`user-agent` headers aren't required; omitting brand/platform returns 400 and omitting origin/referer gets a 403. daft IDs are prefixed `daft-` in `sent_ids.txt` so they can coexist with rent.ie IDs.
- Both search APIs lack full descriptions; Slack uses `<numBedrooms>[ · <numBathrooms>] · <propertyType>`. The rent.ie `property.json` / daft property endpoints have descriptions but cost one extra request per new listing.
- Search API `publishDate` is epoch milliseconds and is converted to a UTC `YYYY-MM-DD` date.
- A 403 from rent.ie or daft.ie is likely Cloudflare; the rent.ie error in `main.rs` suggests retrying from a residential IP.
- `Cargo.toml` forbids `unsafe_code` and denies clippy `unwrap_used`, `expect_used`, `panic`, `indexing_slicing`, `todo`, and `unimplemented`; follow the existing `Result`/`Option` style.
- `RENT_IE_URL`/`SLACK_WEBHOOK_URL` are secrets: `check_rent_ie.service` holds placeholders, and untracked `.vscode/launch.json` holds concrete values. Never copy either into tracked files, docs, or logs. `DAFT_IE_REQUEST` is search configuration, not a secret.
- `Cargo.lock` is committed and CI uses `--locked`; change dependencies with Cargo rather than hand-editing the lockfile.
