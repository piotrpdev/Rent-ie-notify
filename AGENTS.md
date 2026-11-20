# AGENTS.md

Single-binary Rust app (edition 2024): fetches rent.ie, daft.ie, and/or property.ie search results from their JSON APIs, groups cross-site duplicates, dedupes against `sent_ids.txt`, and posts new ones to a Slack webhook. Runs hourly via systemd; the GitHub Actions workflow is manual-only right now.

## Layout

- `src/main.rs` — orchestration: env vars, concurrent source fetching, grouping, dedupe, property details, Slack POST, state file.
- `src/next_data.rs` — Next.js helpers shared by rent.ie and property.ie: `__NEXT_DATA__` extraction (via `tl`), `_next/data` route resolution, `Paging`, slug extraction.
- `src/state.rs` — loads/appends (and syncs) `sent_ids.txt` (legacy ID normalization, `HashSet` lookups) and takes the per-run lock.
- `src/http.rs` — `send_text` (send + status check + body) and `paginate` (sequential page loop with `delay::sleep()`, capped at 50 pages and stopped if the cursor doesn't advance), shared by every source.
- `src/delay.rs` — jittered delay helper for paginated/detail requests.
- `src/listing.rs` — shared `Listing` model (detail-endpoint fields nested in `ListingDetails`), coordinates, image-key extraction, and description/date formatting.
- `src/places.rs` — parses `PLACES_OF_INTEREST`, computes haversine distances, and formats them for Slack.
- `src/routing.rs` — batched OpenRouteService `driving-car` Matrix requests for drive time/distance, with straight-line fallback.
- `src/text.rs` — generates the optional text report grouped by bedroom configuration.
- `src/dedupe.rs` — groups listings across sources by shared numeric ID or intersecting image keys.
- `src/rent_ie.rs` — fetches rent.ie search pages and property details (`fetch_listings`, `fetch_build_id`, `add_listing_details`); builds the rent.ie `_next/data` search API URL from `__NEXT_DATA__.page`/`query` and parses the API JSON; `nanoserde` parses JSON.
- `src/daft_ie.rs` — fetches (`fetch_listings`) and parses daft.ie gateway API responses; links use `https://www.daft.ie<seoFriendlyPath>`.
- `src/property_ie.rs` — fetches (`fetch_listings`) and builds property.ie `_next/data` search URLs from `__NEXT_DATA__` metadata and parses its search JSON. All outbound HTTP uses reqwest blocking; property.ie requires HTTP/2.
- `src/slack.rs` — Slack Block Kit payload built with `nanoserde` (scraped text is mrkdwn-escaped), plus `Sender`, which spaces messages ~1.1 s apart, waits out 429s (`Retry-After`, max 3 retries), and retries a rejected message once without images.
- `src/test_support.rs` — `#[cfg(test)]` listing builder and local HTTP sink (`run_sink`) shared by unit tests.
- `scripts/check.sh` — runs fmt, clippy, unit tests, archive tests, and the release build.
- `examples/slack_sink_e2e.rs` — runs the real binary against a local Slack sink in a temporary working directory; `cargo slack-sink-e2e` is the alias.
- `archives/rentie/` / `archives/daftie/` / `archives/propertyie/` — saved API JSON responses used by the ignored archive tests; not read at runtime.
- `check_rent_ie.service` / `check_rent_ie.timer` — hourly systemd deployment (`Type=oneshot`, no restart-on-failure, 30 min timeout); hardcodes user and repo/binary paths and loads config from `EnvironmentFile=/etc/rent-ie-notify.env`.
- `rent-ie-notify.env.example` — placeholder template for that environment file.
- `.github/workflows/check.yaml` — runs `scripts/check.sh` on Rust 1.100.0 for every push and pull request.
- `.github/workflows/hourly_listings.yaml` — builds/runs with `cargo run --locked --release`; restores `sent_ids.txt` from the newest `sent_ids.txt` artifact of any run via the GitHub API (download-artifact only sees the current run) and re-uploads it even on failure; secrets come from repo secrets, other config from repo variables; `schedule` is commented out.

## Commands

- Check: `cargo fmt --check && cargo clippy --locked --all-targets -- -D warnings && cargo test --locked` (unit tests plus ignored archive-backed tests).
- Check everything: `scripts/check.sh` (also runs the archive tests and release build).
- Archive check: `cargo test --locked -- --ignored` (parses every fixture in `archives/`).
- Local Slack E2E: `cargo slack-sink-e2e` (uses `RENT_IE_URL`/`DAFT_IE_REQUEST`/`PROPERTY_IE_URL`) or pass `--daft-request-file`; `cargo run --example slack_sink_e2e -- ...` works too. ORS is disabled by default; add `--with-routing` only when testing routing.
- Build: `cargo build --release` → `target/release/Rent-ie-notify`.
- Run from the repo root with `SLACK_WEBHOOK_URL` and at least one (non-blank; blank values count as unset) of `RENT_IE_URL`, `DAFT_IE_REQUEST`, or `PROPERTY_IE_URL` set. Optional `PLACES_OF_INTEREST` adds distance lines; optional `OPENROUTESERVICE_API_KEY` enables driving times/distances; optional `TEXT_REPORT_DIR` writes grouped timestamped text reports; optional `MAX_BEDS` caps bedrooms across all sources.
- Rust 1.100.0 is pinned by `rust-toolchain.toml` (rustup installs it automatically) and by both workflows; `Cargo.toml` sets `rust-version = "1.100"` to match. Bump all of these together (plus the rustup line in README.md).

## Gotchas

- `sent_ids.txt` is gitignored runtime state, read/written relative to the current directory. Running elsewhere moves the state. A missing file treats every fetched listing as new; deleting it re-sends everything. Any other read error (permissions, invalid UTF-8, a directory) aborts the run before fetching. Non-dry runs hold an exclusive lock on `sent_ids.txt.lock` (gitignored, `File::try_lock`) for the whole run; a second concurrent run in the same directory exits immediately.
- There is no argument parsing. Without `DRY_RUN=1`, any `cargo run` with valid env vars hits the configured APIs, Slack, and appends IDs. `DRY_RUN=1` still fetches listings/details (and ORS if keyed) and writes reports, but prints the Slack payloads instead of posting and never writes `sent_ids.txt`; `SLACK_WEBHOOK_URL` is not required. `SEED_SENT_IDS=1` records every currently unsent group's IDs without posting (no details, reports, or Slack) to avoid a flood on first run or after losing the state file; combined with `DRY_RUN=1` it only prints the count.
- Slack messages carry 20 listings each (Block Kit allows 50 blocks). Each message's group IDs are appended to `sent_ids.txt` only after Slack returns 2xx, so a failure neither loses nor re-sends other messages. Messages are spaced ~1.1 s apart (Slack webhooks allow ~1/s); a 429 is waited out using `Retry-After` (capped at 60 s, up to 3 retries) before counting as a failure. A rejected (non-429) message is retried once without image accessories (Slack rejects the whole message if an image can't be downloaded); if still rejected, it is skipped, retried next run, and the run exits non-zero. Empty runs are skipped.
- rent.ie listing data comes from `/_next/data/<buildId>/search/<rest>.json?...`. The `buildId` changes on deployments and isn't available from a stable endpoint, so the configured search page is still fetched once per run to read it from `<script id="__NEXT_DATA__">`.
- `rent_ie::search_api_url` derives the API route and query parameters from `__NEXT_DATA__.page`/`query`; don't hand-build it from the public URL, which omits the county/area split. Filters such as max rent (`rent_0-650`) are additional `rest` path segments, not special-cased. The API ignores `pageSize`; pagination follows `pageProps.paging.currentPage/totalPages` with `&page=N`.
- rent.ie and daft.ie share the numeric listing ID space; both are stored as numeric IDs in `sent_ids.txt`. Legacy `<slug>/<id>` and `daft-<id>` entries are normalized when loaded. A group is skipped if any of its IDs has already been sent, and all IDs in a new group are saved.
- Image keys are decoded from the base64 media URL descriptor's `key` field. Duplicates are grouped across sources by shared ID or intersecting image keys; image-key matches within the same source are ignored so reused/stock photos do not merge distinct ads. The daft.ie listing is preferred as the Slack/report representative; otherwise the first listing in the group is used.
- rent.ie `seoFriendlyPath` is a `/share/<slug>/<id>` path that 404s. Links are constructed as `https://www.rent.ie/<pageProps.propertyType>/<slug>/<id>/`; the section varies (`rooms-to-rent`, `houses-to-let`, etc.).
- daft.ie uses a POST to `https://gateway.daft.ie/api/v2/ads/listings`. `DAFT_IE_REQUEST` is the JSON request body; it needs `accept`/`content-type`, `brand: daft`, `platform: web`, `origin`, and `referer` (set in `daft_ie::fetch_listings`). Browser-only `sec-*`/`user-agent` headers aren't required; omitting brand/platform returns 400 and omitting origin/referer gets a 403. The gateway caps `pageSize` at 50 and pagination follows `paging.nextFrom`; `request_body_with_offset` sets `from` inside the top-level `paging` object only (string-aware scan, inserted if missing), so `ranges[].from` filters are never touched.
- All HTTP calls use reqwest blocking. property.ie requires HTTP/2; its HTML/API requests return 403 over HTTP/1.1. Property details/coordinates are fetched through the rent.ie property endpoint, which also accepts property.ie IDs. Its `seoFriendlyPath` is `/for-rent/<slug>/<id>`; public links are built as `https://www.property.ie/<adType>/<slug>/<id>/` with leading/trailing hyphens trimmed.
- property.ie has no working max-bed URL filter (`beds_4-5` redirects to `beds_4`). `MAX_BEDS` is applied after fetching, using `Listing::bedrooms`, parsed from the API `numBedrooms` value; room-share listings without a numeric count are retained.
- All search APIs lack full descriptions; Slack uses `<numBedrooms>[ · <numBathrooms>] · <propertyType>`.
- For each new group representative, `rent_ie::property_details_url`/`extract_details_from_json` fetch `propertyOverview` from the rent.ie property endpoint (it works for daft.ie and property.ie IDs too). Missing values omit the `Available from`/`Minimum lease` lines; individual detail failures are logged and do not fail the run.
- All configured sources are fetched concurrently on separate threads. A failing source is logged and skipped; the run continues with the others (safe because a late-arriving duplicate groups with the already-sent listing) and exits non-zero at the end. The run aborts only if every configured source fails. Pagination and property-detail requests stay sequential and call `delay::sleep()` (1 second plus up to 500 ms of jitter) between requests to avoid rate limiting/bot detection.
- Search API `publishDate` is epoch milliseconds and is converted to a UTC human-readable date such as `14th of November, 2026`.
- `PLACES_OF_INTEREST` is an optional JSON array:
  `[{"name":"Work","latitude":52.244898,"longitude":-7.159315}]`.
  Slack shows straight-line distances from the listing's media/API location when no ORS data is available. Only daft.ie search results currently include coordinates; the dedupe representative borrows them from the daft.ie group member. Listings with no coordinates (including rent-only listings) omit the distance line.
- `OPENROUTESERVICE_API_KEY` enables the `driving-car` matrix profile. Requests are batched (40 origins max), request both duration and distance, and any API failure/limit falls back to straight-line distances. ORS uses OSM profile speeds, not live traffic.
- `TEXT_REPORT_DIR` enables timestamped text reports (`report-YYYY-MM-DD_HH-MM-SS.txt`) of **new listings** by default, grouped by single/double-bed counts from property details. `TEXT_REPORT_INCLUDE_ALL=1` changes it to all unique listings in the current run; that mode fetches details for every unique listing, not just new ones, so only enable it when the extra requests are acceptable. Empty reports are not written, and report write failures are logged without aborting the run.
- `RENT_IE_URL`/`SLACK_WEBHOOK_URL`/`OPENROUTESERVICE_API_KEY` are secrets: `rent-ie-notify.env.example` holds placeholders, the deployed `/etc/rent-ie-notify.env` (0600, root) and untracked `.vscode/launch.json` hold concrete values. Never copy them into tracked files, docs, or logs. reqwest errors embed the request URL, so wrap them with `.map_err(http::without_url)` before adding context. `PLACES_OF_INTEREST` and `DAFT_IE_REQUEST` are configuration, not secrets.
- A 403 from rent.ie or daft.ie is likely Cloudflare; `http::send_text` adds a hint suggesting a residential IP to any 403 error.
- nanoserde-derive panics on `pub(crate)` structs; derived structs must be private or `pub`.
- Errors use `anyhow` (`anyhow::Result`, `.context()`/`.with_context()`, `bail!`). Print errors with `{error:#}` so the cause chain is included; plain `{error}` shows only the outermost context.
- `Cargo.toml` forbids `unsafe_code` and denies clippy `unwrap_used`, `expect_used`, `panic`, `indexing_slicing`, `todo`, and `unimplemented`; follow the existing `Result`/`Option` style.
- `Cargo.lock` is committed and CI uses `--locked`; change dependencies with Cargo rather than hand-editing the lockfile.
