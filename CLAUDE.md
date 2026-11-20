# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

The canonical project guide is `AGENTS.md` (layout, commands, gotchas, secrets policy). It is imported here so it stays the single source of truth — update it, not this file, when project facts change:

@AGENTS.md

## Running individual tests

- Single test or name filter: `cargo test --locked <name_substring>` (e.g. `cargo test --locked dedupe`).
- Tests in one module: `cargo test --locked rent_ie::` (tests live in `#[cfg(test)]` modules inside each `src/*.rs`).
- One archive-backed test: `cargo test --locked <name> -- --ignored`.

## Run pipeline (`pipeline::run` and the source modules)

1. `Config::from_env` (`config.rs`) reads and validates every variable. Non-dry runs take the `sent_ids.txt.lock` run lock, then `state::load` reads `sent_ids.txt` (a missing file is empty; any other read error aborts before fetching).
2. `fetch_all_sources` spawns one thread per configured source (`rent_ie`, `daft_ie`, `property_ie`, each via `fetch_listings` with a base URL from `Endpoints`); each paginates through `http::paginate`, and every site's results are parsed through the shared `listing::RawListing`. A failed source is logged and skipped (the run exits non-zero at the end); only all-sources-failed aborts. rent.ie also returns the Next.js `buildId` for property-detail URLs (`rent_ie::fetch_build_id` is used if rent.ie isn't configured or failed).
3. `retain_max_beds` applies `MAX_BEDS` using `Listing::bedrooms`.
4. `dedupe::group_listings` groups listings across sources (shared numeric ID or cross-source image key, union-find) and `representative` picks one (daft.ie preferred, borrowing its coordinates).
5. `select_new` drops groups whose IDs intersect `sent_ids.txt` and pairs each new representative with all of its group's IDs. With `SEED_SENT_IDS=1` those IDs are appended and the run ends here.
6. `prepare_details_and_report` fetches property details (`rent_ie::add_listing_details`) for new representatives — or all unique listings with `TEXT_REPORT_INCLUDE_ALL=1` — and writes the optional non-empty `text` report. If nothing is new, the run exits here.
7. `routing::fetch_routes` gets ORS drive times when a key is set (Slack falls back to `places` haversine distances); `send_slack_listings` posts Block Kit payloads in chunks of 20 through `slack::Sender` (paced ~1.1 s apart, waits out 429s) and appends and syncs each chunk's group IDs to `sent_ids.txt` once Slack accepts it (`DRY_RUN=1` prints the payloads and writes nothing). Every exit path prints a one-line `Summary:` before returning (non-zero if any source failed or Slack rejected a message).

Errors are `anyhow`; print them with `{error:#}`.

Background on the dedupe design: `docs/cross-site-listing-deduplication.md`.
