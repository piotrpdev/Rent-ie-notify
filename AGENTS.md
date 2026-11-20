# AGENTS.md

Single-binary Rust app (edition 2024): fetches rent.ie, daft.ie, and/or property.ie search results from their JSON APIs, groups cross-site duplicates, dedupes against `sent_ids.txt`, and posts new ones to a Slack webhook. Runs hourly via systemd; the GitHub Actions workflow is manual-only right now.

## Layout

- `src/main.rs` — orchestration: env vars, source fetching, grouping, dedupe, property details, Slack POST, state file.
- `src/delay.rs` — jittered delay helper for paginated/detail requests.
- `src/listing.rs` — shared `Listing` model, coordinates, image-key extraction, and description/date formatting.
- `src/places.rs` — parses `PLACES_OF_INTEREST`, computes haversine distances, and formats them for Slack.
- `src/routing.rs` — batched OpenRouteService `driving-car` Matrix requests for drive time/distance, with straight-line fallback.
- `src/text.rs` — generates the optional text report grouped by bedroom configuration.
- `src/dedupe.rs` — groups listings across sources by shared numeric ID or intersecting image keys.
- `src/rent_ie.rs` — builds the rent.ie `_next/data` search API URL from `__NEXT_DATA__.page`/`query` and parses the API JSON; `tl` locates the metadata script, `nanoserde` parses JSON.
- `src/daft_ie.rs` — parses daft.ie gateway API responses; links use `https://www.daft.ie<seoFriendlyPath>`.
- `src/property_ie.rs` — builds property.ie `_next/data` search URLs from `__NEXT_DATA__` metadata and parses its search JSON. All outbound HTTP uses reqwest blocking; property.ie requires HTTP/2.
- `src/slack.rs` — Slack Block Kit payload built with `nanoserde`.
- `scripts/check.sh` — runs fmt, clippy, unit tests, archive tests, and the release build.
- `examples/slack_sink_e2e.rs` — runs the real binary against a local Slack sink in a temporary working directory; `cargo slack-sink-e2e` is the alias.
- `archives/rentie/` / `archives/daftie/` — saved API JSON responses for local verification; not read at runtime.
- `check_rent_ie.service` / `check_rent_ie.timer` — hourly systemd deployment; hardcodes user, repo/binary paths, and env vars.
- `.github/workflows/hourly_listings.yaml` — builds/runs with `cargo run --locked --release`; persists `sent_ids.txt` as a workflow artifact; `schedule` is commented out.

## Commands

- Check: `cargo fmt --check && cargo clippy --locked --all-targets -- -D warnings && cargo test --locked` (33 unit tests, plus 3 ignored archive-backed tests).
- Check everything: `scripts/check.sh` (also runs the archive tests and release build).
- Archive check: `cargo test --locked -- --ignored` (parses the rent.ie and daft.ie fixtures in `archives/`).
- Local Slack E2E: `cargo slack-sink-e2e` (uses `RENT_IE_URL`/`DAFT_IE_REQUEST`/`PROPERTY_IE_URL`) or pass `--daft-request-file`; `cargo run --example slack_sink_e2e -- ...` works too. ORS is disabled by default; add `--with-routing` only when testing routing.
- Build: `cargo build --release` → `target/release/Rent-ie-notify`.
- Run from the repo root with `SLACK_WEBHOOK_URL` and at least one of `RENT_IE_URL`, `DAFT_IE_REQUEST`, or `PROPERTY_IE_URL` set. Optional `PLACES_OF_INTEREST` adds distance lines; optional `OPENROUTESERVICE_API_KEY` enables driving times/distances; optional `TEXT_REPORT_DIR` writes grouped timestamped text reports; optional `MAX_BEDS` caps bedrooms across all sources.
- Rust 1.92.0 is pinned by `rust-toolchain.toml` (rustup installs it automatically) and by the workflows; `Cargo.toml` sets `rust-version = "1.92"` so newer clippy releases (`cargo +stable clippy`) don't suggest APIs that 1.92 lacks. `clippy::question_mark` is allowed because nanoserde's derive output trips it on clippy 1.92.

## Gotchas

- `sent_ids.txt` is gitignored runtime state, read/written relative to the current directory. Running elsewhere moves the state. A missing file treats every fetched listing as new; deleting it re-sends everything.
- There is no dry-run or argument parsing: any `cargo run` with valid env vars hits the configured APIs, Slack, and appends IDs. Use a throwaway webhook for end-to-end checks.
- Slack's HTTP status is not checked; a transport-level success is enough to mark listings as sent, so a rejected payload can be lost silently. New groups are sent in chunks of 20 listings to stay under Slack's Block Kit limit; empty runs are skipped.
- rent.ie listing data comes from `/_next/data/<buildId>/search/<rest>.json?...`. The `buildId` changes on deployments and isn't available from a stable endpoint, so the configured search page is still fetched once per run to read it from `<script id="__NEXT_DATA__">`.
- `rent_ie::search_api_url` derives the API route and query parameters from `__NEXT_DATA__.page`/`query`; don't hand-build it from the public URL, which omits the county/area split. Filters such as max rent (`rent_0-650`) are additional `rest` path segments, not special-cased. The API ignores `pageSize`; pagination follows `pageProps.paging.currentPage/totalPages` with `&page=N`.
- rent.ie and daft.ie share the numeric listing ID space; both are stored as numeric IDs in `sent_ids.txt`. Legacy `<slug>/<id>` and `daft-<id>` entries are normalized when loaded. A group is skipped if any of its IDs has already been sent, and all IDs in a new group are saved.
- Image keys are decoded from the base64 media URL descriptor's `key` field. Duplicates are grouped across sources by shared ID or intersecting image keys; image-key matches within the same source are ignored so reused/stock photos do not merge distinct ads. The daft.ie listing is preferred as the Slack/report representative; otherwise the first listing in the group is used.
- rent.ie `seoFriendlyPath` is a `/share/<slug>/<id>` path that 404s. Links are constructed as `https://www.rent.ie/<pageProps.propertyType>/<slug>/<id>/`; the section varies (`rooms-to-rent`, `houses-to-let`, etc.).
- daft.ie uses a POST to `https://gateway.daft.ie/api/v2/ads/listings`. `DAFT_IE_REQUEST` is the JSON request body; it needs `accept`/`content-type`, `brand: daft`, `platform: web`, `origin`, and `referer` (set in `main.rs`). Browser-only `sec-*`/`user-agent` headers aren't required; omitting brand/platform returns 400 and omitting origin/referer gets a 403. The gateway caps `pageSize` at 50 and pagination follows `paging.nextFrom`, rewriting only the request body's `paging.from` value.
- All HTTP calls use reqwest blocking. property.ie requires HTTP/2; its HTML/API requests return 403 over HTTP/1.1. Property details/coordinates are fetched through the rent.ie property endpoint, which also accepts property.ie IDs. Its `seoFriendlyPath` is `/for-rent/<slug>/<id>`; public links are built as `https://www.property.ie/<adType>/<slug>/<id>/` with leading/trailing hyphens trimmed.
- property.ie has no working max-bed URL filter (`beds_4-5` redirects to `beds_4`). `MAX_BEDS` is applied after fetching, parsing the numeric bedroom count from the listing description; room-share listings without a numeric count are retained.
- All search APIs lack full descriptions; Slack uses `<numBedrooms>[ · <numBathrooms>] · <propertyType>`.
- For each new group representative, `rent_ie::property_details_url`/`extract_details_from_json` fetch `propertyOverview` from the rent.ie property endpoint (it works for daft.ie and property.ie IDs too). Missing values omit the `Available from`/`Minimum lease` lines; individual detail failures are logged and do not fail the run.
- All configured sources are fetched concurrently on separate threads. Pagination and property-detail requests stay sequential and call `delay::sleep()` (1 second plus up to 500 ms of jitter) between requests to avoid rate limiting/bot detection.
- Search API `publishDate` is epoch milliseconds and is converted to a UTC human-readable date such as `14th of November, 2026`.
- `PLACES_OF_INTEREST` is an optional JSON array:
  `[{"name":"Work","latitude":52.244898,"longitude":-7.159315}]`.
  Slack shows straight-line distances from the listing's media/API location when no ORS data is available. Only daft.ie search results currently include coordinates; the dedupe representative borrows them from the daft.ie group member. Listings with no coordinates (including rent-only listings) omit the distance line.
- `OPENROUTESERVICE_API_KEY` enables the `driving-car` matrix profile. Requests are batched (40 origins max), request both duration and distance, and any API failure/limit falls back to straight-line distances. ORS uses OSM profile speeds, not live traffic.
- `TEXT_REPORT_DIR` enables timestamped text reports (`report-YYYY-MM-DD_HH-MM-SS.txt`) of **new listings** by default, grouped by single/double-bed counts from property details. `TEXT_REPORT_INCLUDE_ALL=1` changes it to all unique listings in the current run; that mode fetches details for every unique listing, not just new ones, so only enable it when the extra requests are acceptable.
- `RENT_IE_URL`/`SLACK_WEBHOOK_URL`/`OPENROUTESERVICE_API_KEY` are secrets: `check_rent_ie.service` holds placeholders, and untracked `.vscode/launch.json` holds concrete values. Never copy them into tracked files, docs, or logs. `PLACES_OF_INTEREST` and `DAFT_IE_REQUEST` are configuration, not secrets.
- A 403 from rent.ie or daft.ie is likely Cloudflare; the rent.ie error in `main.rs` suggests retrying from a residential IP.
- `Cargo.toml` forbids `unsafe_code` and denies clippy `unwrap_used`, `expect_used`, `panic`, `indexing_slicing`, `todo`, and `unimplemented`; follow the existing `Result`/`Option` style.
- `Cargo.lock` is committed and CI uses `--locked`; change dependencies with Cargo rather than hand-editing the lockfile.
