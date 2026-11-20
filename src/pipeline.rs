//! One run of the notifier, from fetching sources to recording what was sent.

use std::{collections::HashSet, fmt, path::Path, thread};

use anyhow::{Result, anyhow, bail};

use crate::config::{Config, Sources};
use crate::endpoints::Endpoints;
use crate::listing::Listing;
use crate::{daft_ie, dedupe, places, property_ie, rent_ie, routing, slack, state, text};

// Slack allows at most 50 Block Kit blocks per message. Each listing uses a
// section plus a divider, so 20 listings is 39 blocks, leaving some headroom.
const SLACK_LISTINGS_PER_MESSAGE: usize = 20;

struct FetchedListings {
    listings: Vec<Listing>,
    /// Listings fetched per successful source, for the run summary.
    counts: Vec<(&'static str, usize)>,
    rent_build_id: Option<String>,
    /// One message per configured source that failed.
    failures: Vec<String>,
}

/// Joins a source's fetch thread, recording (and logging) a failure instead of
/// propagating it so the other sources can still be processed.
fn join_source<T>(
    name: &str,
    handle: Option<thread::JoinHandle<Result<T>>>,
    failures: &mut Vec<String>,
) -> Option<T> {
    let result = handle?
        .join()
        .unwrap_or_else(|_| Err(anyhow!("fetch thread panicked")));

    match result {
        Ok(value) => Some(value),
        Err(error) => {
            eprintln!("{name} failed, continuing with the other sources: {error:#}");
            failures.push(format!("{name}: {error:#}"));
            None
        }
    }
}

/// Fetches all configured sources concurrently. Pagination remains sequential
/// within each source so we don't burst the same host with parallel requests.
/// Fails only if every configured source failed.
fn fetch_all_sources(sources: Sources, endpoints: &Endpoints) -> Result<FetchedListings> {
    let Sources {
        rent_ie_url,
        daft_ie_request,
        property_ie_url,
    } = sources;
    let rent_base = endpoints.rent_ie.clone();
    let rent_thread =
        rent_ie_url.map(|url| thread::spawn(move || rent_ie::fetch_listings(&rent_base, &url)));
    let daft_api = endpoints.daft_ie_api.clone();
    let daft_thread = daft_ie_request
        .map(|body| thread::spawn(move || daft_ie::fetch_listings(&daft_api, &body)));
    let property_base = endpoints.property_ie.clone();
    let property_thread = property_ie_url
        .map(|url| thread::spawn(move || property_ie::fetch_listings(&property_base, &url)));
    let configured = [
        rent_thread.is_some(),
        daft_thread.is_some(),
        property_thread.is_some(),
    ]
    .into_iter()
    .filter(|configured| *configured)
    .count();

    let mut failures = Vec::new();
    let rent = join_source("rent.ie", rent_thread, &mut failures);
    let daft = join_source("daft.ie", daft_thread, &mut failures);
    let property = join_source("property.ie", property_thread, &mut failures);

    if failures.len() == configured {
        bail!("All sources failed: {}", failures.join("; "));
    }

    let mut counts = Vec::new();
    let mut listings = Vec::new();
    let rent_build_id = rent.map(|(rent_listings, build_id)| {
        counts.push(("rent.ie", rent_listings.len()));
        listings.extend(rent_listings);
        build_id
    });
    for (name, fetched) in [("daft.ie", daft), ("property.ie", property)] {
        if let Some(fetched) = fetched {
            counts.push((name, fetched.len()));
            listings.extend(fetched);
        }
    }

    Ok(FetchedListings {
        listings,
        counts,
        rent_build_id,
        failures,
    })
}

/// Counts for the one-line summary printed at the end of every run.
#[derive(Debug, Default)]
struct Summary {
    fetched: Vec<(&'static str, usize)>,
    unique: usize,
    new: usize,
    /// What happened to the new listings, e.g. "3 sent".
    outcome: String,
}

impl fmt::Display for Summary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let fetched: Vec<String> = self
            .fetched
            .iter()
            .map(|(source, count)| format!("{source} {count}"))
            .collect();
        let fetched = if fetched.is_empty() {
            "nothing".to_string()
        } else {
            fetched.join(", ")
        };
        write!(
            formatter,
            "Summary: fetched {fetched} → {} unique, {} new; {}",
            self.unique, self.new, self.outcome
        )
    }
}

/// Prints the run summary, then turns failures that were tolerated during
/// the run (failed sources, Slack rejections) into a non-zero exit, so
/// systemd/journald still flag the run.
fn finish(summary: &Summary, failures: &[String]) -> Result<()> {
    let failed = match failures.len() {
        0 => String::new(),
        1 => "; 1 failure".to_string(),
        count => format!("; {count} failures"),
    };
    println!("{summary}{failed}");

    if failures.is_empty() {
        Ok(())
    } else {
        bail!("Run finished with errors: {}", failures.join("; "))
    }
}

/// Returns each ID once, keeping first-seen order.
fn unique_ids<'a>(ids: impl IntoIterator<Item = &'a String>) -> Vec<String> {
    let mut seen = HashSet::new();
    ids.into_iter()
        .filter(|id| seen.insert(id.as_str()))
        .cloned()
        .collect()
}

/// Drops listings with more than `max_beds` bedrooms. Listings without a
/// numeric count (room shares) are kept.
fn retain_max_beds(listings: &mut Vec<Listing>, max_beds: u32) {
    listings.retain(|listing| listing.bedrooms.is_none_or(|bedrooms| bedrooms <= max_beds));
}

/// Returns the representative of each group with no already-sent ID, alongside
/// every ID in that group (all of which are recorded once it is sent).
/// Ordered oldest first by publish date, so the newest listing ends up at the
/// bottom of the Slack channel; listings without a date come last.
fn select_new(
    groups: &[Vec<Listing>],
    sent_ids: &HashSet<String>,
) -> (Vec<Listing>, Vec<Vec<String>>) {
    let mut new: Vec<(Listing, Vec<String>)> = groups
        .iter()
        .filter(|group| !group.iter().any(|listing| sent_ids.contains(&listing.id)))
        .filter_map(|group| {
            let ids = group.iter().map(|listing| listing.id.clone()).collect();
            Some((dedupe::representative(group)?, ids))
        })
        .collect();
    new.sort_by_key(|(listing, _)| (listing.published_at.is_none(), listing.published_at));
    new.into_iter().unzip()
}

/// Where new listings go.
enum Delivery {
    Slack(slack::Sender),
    /// `DRY_RUN=1`: print the Slack payloads and leave `sent_ids.txt` alone.
    DryRun,
}

/// How many listings Slack accepted or rejected.
#[derive(Debug, Default, PartialEq, Eq)]
struct Delivered {
    sent: usize,
    rejected: usize,
}

/// Posts new listings to Slack in chunks and appends each chunk's group IDs to
/// the state file as soon as Slack accepts it, so a later failure neither loses
/// nor re-sends earlier chunks. Rejected chunks are skipped and counted (and
/// retried next run); failing to record state aborts immediately to avoid
/// duplicates.
fn send_slack_listings(
    delivery: &mut Delivery,
    listings: &[Listing],
    group_ids: &[Vec<String>],
    places: &[places::Place],
    routes: &routing::RouteMap,
    sent_ids_path: &Path,
) -> Result<Delivered> {
    let mut delivered = Delivered::default();

    for (chunk, chunk_ids) in listings
        .chunks(SLACK_LISTINGS_PER_MESSAGE)
        .zip(group_ids.chunks(SLACK_LISTINGS_PER_MESSAGE))
    {
        let sender = match delivery {
            Delivery::Slack(sender) => sender,
            Delivery::DryRun => {
                println!(
                    "DRY_RUN: would send {} listings to Slack:\n{}",
                    chunk.len(),
                    slack::listings_to_slack_message_json(chunk, places, routes, true)
                );
                continue;
            }
        };

        println!("Sending {} new listings to Slack...", chunk.len());
        if let Err(error) = sender.send(chunk, places, routes) {
            eprintln!("{error:#}");
            delivered.rejected += chunk.len();
            continue;
        }

        state::append(sent_ids_path, &unique_ids(chunk_ids.iter().flatten()))?;
        delivered.sent += chunk.len();
    }

    Ok(delivered)
}

/// Writes the optional text report. Empty reports are skipped, and failures
/// are logged rather than aborting the run, because the report is optional and
/// Slack notifications should still go out.
fn write_report(report_dir: &str, listings: &[Listing]) {
    if listings.is_empty() {
        return;
    }

    match text::write_report(report_dir, listings) {
        Ok(path) => println!("Wrote report to {path}"),
        Err(error) => eprintln!("{error:#}; continuing without a report."),
    }
}

/// Fetches property details for new listings (or for every unique listing
/// with `TEXT_REPORT_INCLUDE_ALL`) and writes the optional text report.
fn prepare_details_and_report(
    endpoints: &Endpoints,
    groups: &[Vec<Listing>],
    new_listings: &mut [Listing],
    report_dir: Option<&str>,
    include_all: bool,
    rent_build_id: Option<String>,
) {
    match report_dir {
        Some(report_dir) if include_all => {
            let mut all_listings: Vec<Listing> = groups
                .iter()
                .filter_map(|group| dedupe::representative(group))
                .collect();

            populate_listing_details(endpoints, &mut all_listings, rent_build_id);

            for listing in new_listings.iter_mut() {
                if let Some(source) = all_listings
                    .iter()
                    .find(|candidate| candidate.id == listing.id)
                {
                    listing.details.clone_from(&source.details);
                }
            }

            write_report(report_dir, &all_listings);
        }
        report_dir => {
            populate_listing_details(endpoints, new_listings, rent_build_id);
            if let Some(report_dir) = report_dir {
                write_report(report_dir, new_listings);
            }
        }
    }
}

fn populate_listing_details(
    endpoints: &Endpoints,
    listings: &mut [Listing],
    rent_build_id: Option<String>,
) {
    if listings.is_empty() {
        return;
    }

    let details_build_id = rent_build_id.map_or_else(
        || match rent_ie::fetch_build_id(&endpoints.rent_ie) {
            Ok(build_id) => Some(build_id),
            Err(error) => {
                eprintln!("Failed to fetch rent.ie build ID for property details: {error:#}");
                None
            }
        },
        Some,
    );

    if let Some(build_id) = details_build_id {
        rent_ie::add_listing_details(&endpoints.rent_ie, listings, &build_id);
    }
}

/// `SEED_SENT_IDS`: records every unsent group's IDs without posting (or,
/// in a dry run, only reports how many it would record). Returns the summary
/// outcome.
fn seed(group_ids: &[Vec<String>], sent_ids_path: &Path, dry_run: bool) -> Result<String> {
    let ids = unique_ids(group_ids.iter().flatten());
    if dry_run {
        println!("DRY_RUN: would seed {} listing IDs.", ids.len());
        return Ok(format!("would seed {} IDs (dry run)", ids.len()));
    }

    println!(
        "SEED_SENT_IDS is set: recording {} listing IDs without posting...",
        ids.len()
    );
    state::append(sent_ids_path, &ids)?;
    Ok(format!("{} IDs seeded", ids.len()))
}

/// Runs one check: fetch, group, filter already-sent listings, fetch details,
/// write the report, post to Slack, and record what was sent. State files
/// (`sent_ids.txt`, its lock) live in `state_dir`.
pub(crate) fn run(config: Config, endpoints: &Endpoints, state_dir: &Path) -> Result<()> {
    let Config {
        sources,
        webhook_url,
        ors_api_key,
        text_report_dir,
        text_report_include_all,
        max_beds,
        places,
        dry_run,
        seed_only,
        warnings,
    } = config;
    for warning in warnings {
        eprintln!("Warning: {warning}.");
    }
    if dry_run {
        println!("DRY_RUN is set: Slack payloads are printed and sent_ids.txt is not updated.");
    }

    // Held until main returns. Dry runs never write state, so they don't lock.
    let _lock = if dry_run {
        None
    } else {
        Some(state::lock(&state_dir.join(state::LOCK_FILENAME))?)
    };

    // Load state before fetching so an unreadable file fails fast.
    println!("Loading already sent listing IDs from file...");
    let sent_ids_path = state_dir.join(state::SENT_IDS_FILENAME);
    let sent_ids = state::load(&sent_ids_path)?;

    let FetchedListings {
        mut listings,
        counts,
        rent_build_id,
        mut failures,
    } = fetch_all_sources(sources, endpoints)?;
    let mut summary = Summary {
        fetched: counts,
        ..Summary::default()
    };

    if let Some(max_beds) = max_beds {
        retain_max_beds(&mut listings, max_beds);
    }

    let listing_count = listings.len();
    let groups = dedupe::group_listings(listings);
    println!(
        "Grouped {listing_count} listings into {} unique listings.",
        groups.len()
    );

    let (mut new_listings, new_group_ids) = select_new(&groups, &sent_ids);
    summary.unique = groups.len();
    summary.new = new_listings.len();

    if seed_only {
        summary.outcome = seed(&new_group_ids, &sent_ids_path, dry_run)?;
        return finish(&summary, &failures);
    }

    prepare_details_and_report(
        endpoints,
        &groups,
        &mut new_listings,
        text_report_dir.as_deref(),
        text_report_include_all,
        rent_build_id,
    );

    if new_listings.is_empty() {
        summary.outcome = "nothing to send".to_string();
        return finish(&summary, &failures);
    }

    let routes = ors_api_key
        .as_deref()
        .map_or_else(routing::RouteMap::new, |api_key| {
            routing::fetch_routes(&endpoints.ors_matrix, &new_listings, &places, api_key)
        });

    let mut delivery = match webhook_url {
        Some(webhook_url) if !dry_run => Delivery::Slack(slack::Sender::new(webhook_url)),
        _ => Delivery::DryRun,
    };
    let delivered = send_slack_listings(
        &mut delivery,
        &new_listings,
        &new_group_ids,
        &places,
        &routes,
        &sent_ids_path,
    )?;
    summary.outcome = if matches!(delivery, Delivery::DryRun) {
        format!("would send {} (dry run)", new_listings.len())
    } else {
        format!("{} sent", delivered.sent)
    };
    if delivered.rejected > 0 {
        failures.push(format!(
            "Slack did not accept {} listings; they will be retried next run",
            delivered.rejected
        ));
    }

    finish(&summary, &failures)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use anyhow::{Context as _, Result, anyhow};

    use super::{
        Delivered, Delivery, Summary, fetch_all_sources, retain_max_beds, run, select_new,
        send_slack_listings,
    };
    use crate::config::{Config, Sources};
    use crate::endpoints::Endpoints;
    use crate::listing::{Listing, ListingSource};
    use crate::routing::RouteMap;
    use crate::test_support::{listing, run_server, run_sink};
    use crate::{slack, state};

    fn ids(listings: &[Listing]) -> Vec<&str> {
        listings.iter().map(|listing| listing.id.as_str()).collect()
    }

    #[test]
    fn max_beds_keeps_room_shares() {
        let mut listings = vec![
            Listing {
                bedrooms: Some(2),
                ..listing("1", ListingSource::Rent)
            },
            Listing {
                bedrooms: Some(5),
                ..listing("2", ListingSource::Rent)
            },
            listing("3", ListingSource::Rent),
        ];

        retain_max_beds(&mut listings, 4);

        assert_eq!(ids(&listings), vec!["1", "3"]);
    }

    #[test]
    fn orders_new_listings_oldest_first_with_undated_last() {
        let dated = |id: &str, published_at: Option<u64>| {
            vec![Listing {
                published_at,
                ..listing(id, ListingSource::Daft)
            }]
        };
        let groups = vec![
            dated("new", Some(300)),
            dated("undated", None),
            dated("old", Some(100)),
            dated("middle", Some(200)),
        ];

        let (new_listings, _) = select_new(&groups, &HashSet::new());

        assert_eq!(ids(&new_listings), vec!["old", "middle", "new", "undated"]);
    }

    #[test]
    fn selects_unsent_groups_with_all_their_ids() {
        let groups = vec![
            vec![
                listing("1", ListingSource::Rent),
                listing("2", ListingSource::Daft),
            ],
            vec![
                listing("3", ListingSource::Rent),
                listing("4", ListingSource::Property),
            ],
            vec![listing("5", ListingSource::Property)],
        ];
        let sent_ids: HashSet<String> = HashSet::from(["4".to_string()]);

        let (new_listings, group_ids) = select_new(&groups, &sent_ids);

        assert_eq!(ids(&new_listings), vec!["2", "5"]);
        assert_eq!(group_ids, vec![vec!["1", "2"], vec!["5"]]);
    }

    fn numbered(count: usize) -> (Vec<Listing>, Vec<Vec<String>>) {
        (1..=count)
            .map(|index| {
                let id = index.to_string();
                (listing(&id, ListingSource::Daft), vec![id.clone(), id])
            })
            .unzip()
    }

    #[test]
    fn records_ids_only_for_messages_slack_accepts() -> Result<()> {
        // 45 listings -> messages of 20, 20, 5. The second message is rejected
        // (no images, so no retry); the others are accepted.
        let (listings, group_ids) = numbered(45);
        let (url, sink) = run_sink(vec![200, 400, 200])?;
        let directory = tempfile::tempdir().context("failed to create temp dir")?;
        let path = directory.path().join("sent_ids.txt");

        let delivered = send_slack_listings(
            &mut Delivery::Slack(slack::Sender::with_sleep(url, |_| {})),
            &listings,
            &group_ids,
            &[],
            &RouteMap::new(),
            &path,
        )?;

        assert_eq!(
            delivered,
            Delivered {
                sent: 25,
                rejected: 20
            }
        );
        assert_eq!(sink.join().map_err(|_| anyhow!("sink panicked"))?.len(), 3);
        let content = std::fs::read_to_string(&path)?;
        let saved: Vec<&str> = content.lines().collect();
        let expected: Vec<String> = (1..=20).chain(41..=45).map(|id| id.to_string()).collect();
        assert_eq!(saved, expected);
        assert_eq!(state::load(&path)?.len(), 25);
        Ok(())
    }

    #[test]
    fn dry_run_posts_nothing_and_writes_no_state() -> Result<()> {
        let (listings, group_ids) = numbered(3);
        let directory = tempfile::tempdir().context("failed to create temp dir")?;
        let path = directory.path().join("sent_ids.txt");

        send_slack_listings(
            &mut Delivery::DryRun,
            &listings,
            &group_ids,
            &[],
            &RouteMap::new(),
            &path,
        )?;

        assert!(!path.exists());
        Ok(())
    }

    /// Nothing listens on port 1, so requests there fail immediately.
    const DEAD_URL: &str = "http://127.0.0.1:1/search";

    #[test]
    fn keeps_going_when_one_source_fails() -> Result<()> {
        let daft_page = r#"{"listings":[{"listing":{"id":7,"title":"Listing 7","price":"€1","seoFriendlyPath":"/for-rent/slug/7"}}]}"#;
        let (daft_api, server) = run_server(vec![(200, daft_page.to_string())])?;
        let endpoints = Endpoints {
            daft_ie_api: daft_api,
            ..Endpoints::default()
        };
        let sources = Sources {
            rent_ie_url: Some(DEAD_URL.to_string()),
            daft_ie_request: Some(r#"{"paging":{"from":"0"}}"#.to_string()),
            property_ie_url: None,
        };

        let fetched = fetch_all_sources(sources, &endpoints)?;

        assert_eq!(ids(&fetched.listings), vec!["7"]);
        assert_eq!(fetched.counts, vec![("daft.ie", 1)]);
        assert!(fetched.rent_build_id.is_none());
        assert_eq!(fetched.failures.len(), 1);
        assert!(
            fetched
                .failures
                .iter()
                .all(|failure| failure.starts_with("rent.ie: "))
        );
        server.join().map_err(|_| anyhow!("server panicked"))?;
        Ok(())
    }

    #[test]
    fn fails_when_every_source_fails() {
        let sources = Sources {
            rent_ie_url: Some(DEAD_URL.to_string()),
            property_ie_url: Some(DEAD_URL.to_string()),
            ..Sources::default()
        };

        let result = fetch_all_sources(sources, &Endpoints::default());

        assert!(result.is_err_and(|error| error.to_string().starts_with("All sources failed")));
    }

    // End-to-end runs of `run` against local fake sites and a Slack sink.

    /// rent.ie `__NEXT_DATA__` search page.
    const RENT_SEARCH_HTML: &str = r#"<html><head><script id="__NEXT_DATA__" type="application/json">{"buildId":"build","page":"/search/[[...rest]]","query":{"propertyType":"houses-to-let","county":"waterford","rest":["waterford-city"]}}</script></head></html>"#;
    const DETAILS_JSON: &str = r#"{"pageProps":{"listing":{"propertyOverview":[{"label":"Available From","text":"Now"}]}}}"#;
    const DAFT_REQUEST: &str = r#"{"paging":{"from":"0","pageSize":"50"}}"#;

    fn raw_listing(id: u32, path_prefix: &str) -> String {
        format!(
            r#"{{"listing":{{"id":{id},"title":"Listing {id}","price":"€1,000 per month","numBedrooms":"2 Bed","seoFriendlyPath":"{path_prefix}/slug-{id}/{id}"}}}}"#
        )
    }

    fn rent_page(ids: &[u32]) -> String {
        let listings: Vec<String> = ids.iter().map(|id| raw_listing(*id, "/share")).collect();
        format!(
            r#"{{"pageProps":{{"propertyType":"houses-to-let","listings":[{}]}}}}"#,
            listings.join(",")
        )
    }

    fn daft_page(ids: &[u32]) -> String {
        let listings: Vec<String> = ids.iter().map(|id| raw_listing(*id, "/for-rent")).collect();
        format!(r#"{{"listings":[{}]}}"#, listings.join(","))
    }

    fn config(sources: Sources) -> Config {
        Config {
            sources,
            webhook_url: None,
            ors_api_key: None,
            text_report_dir: None,
            text_report_include_all: false,
            max_beds: None,
            places: Vec::new(),
            dry_run: false,
            seed_only: false,
            warnings: Vec::new(),
        }
    }

    fn saved_ids(state_dir: &std::path::Path) -> Result<Vec<String>> {
        let content = std::fs::read_to_string(state_dir.join(state::SENT_IDS_FILENAME))?;
        Ok(content.lines().map(ToString::to_string).collect())
    }

    #[test]
    fn runs_end_to_end_and_skips_already_sent_listings() -> Result<()> {
        // rent.ie lists 10 and 11; daft.ie lists 11 (same ad) and 12.
        // Three groups, so three detail requests on the rent.ie server.
        let (rent, rent_server) = run_server(vec![
            (200, RENT_SEARCH_HTML.to_string()),
            (200, rent_page(&[10, 11])),
            (200, DETAILS_JSON.to_string()),
            (200, DETAILS_JSON.to_string()),
            (200, DETAILS_JSON.to_string()),
        ])?;
        let (daft, daft_server) = run_server(vec![(200, daft_page(&[11, 12]))])?;
        let (webhook, sink) = run_sink(vec![200])?;
        let endpoints = Endpoints {
            rent_ie: rent.clone(),
            daft_ie_api: daft,
            ..Endpoints::default()
        };
        let sources = || Sources {
            rent_ie_url: Some(format!("{rent}/search")),
            daft_ie_request: Some(DAFT_REQUEST.to_string()),
            property_ie_url: None,
        };
        let state_dir = tempfile::tempdir()?;

        run(
            Config {
                webhook_url: Some(webhook),
                ..config(sources())
            },
            &endpoints,
            state_dir.path(),
        )?;

        let messages = sink.join().map_err(|_| anyhow!("sink panicked"))?;
        assert_eq!(messages.len(), 1);
        let message = messages.first().context("expected a Slack message")?;
        assert_eq!(message.matches(r#""type":"section""#).count(), 3);
        assert!(message.contains("*Available from:* Now"));
        // The daft.ie copy represents the shared listing.
        assert!(message.contains("https://www.daft.ie/for-rent/slug-11/11"));
        assert!(
            message.contains("*Also on:* <https://www.rent.ie/houses-to-let/slug-11/11/|rent.ie>")
        );
        assert_eq!(saved_ids(state_dir.path())?, vec!["10", "11", "12"]);
        assert_eq!(
            rent_server
                .join()
                .map_err(|_| anyhow!("server panicked"))?
                .len(),
            5
        );
        daft_server.join().map_err(|_| anyhow!("server panicked"))?;

        // A second run finds nothing new: no details, no Slack message (the
        // closed sink would make any post fail the run).
        let (rent, _rent_server) = run_server(vec![
            (200, RENT_SEARCH_HTML.to_string()),
            (200, rent_page(&[10, 11])),
        ])?;
        let (daft, _daft_server) = run_server(vec![(200, daft_page(&[11, 12]))])?;
        let (webhook, _sink) = run_sink(Vec::new())?;
        let endpoints = Endpoints {
            rent_ie: rent.clone(),
            daft_ie_api: daft,
            ..Endpoints::default()
        };
        run(
            Config {
                webhook_url: Some(webhook),
                ..config(Sources {
                    rent_ie_url: Some(format!("{rent}/search")),
                    ..sources()
                })
            },
            &endpoints,
            state_dir.path(),
        )?;
        assert_eq!(saved_ids(state_dir.path())?.len(), 3);
        Ok(())
    }

    #[test]
    fn seeding_records_ids_without_posting() -> Result<()> {
        let (daft, server) = run_server(vec![(200, daft_page(&[1, 2]))])?;
        let endpoints = Endpoints {
            daft_ie_api: daft,
            ..Endpoints::default()
        };
        let state_dir = tempfile::tempdir()?;

        run(
            Config {
                seed_only: true,
                ..config(Sources {
                    daft_ie_request: Some(DAFT_REQUEST.to_string()),
                    ..Sources::default()
                })
            },
            &endpoints,
            state_dir.path(),
        )?;

        assert_eq!(saved_ids(state_dir.path())?, vec!["1", "2"]);
        assert_eq!(
            server.join().map_err(|_| anyhow!("server panicked"))?.len(),
            1
        );
        Ok(())
    }

    #[test]
    fn dry_run_writes_no_state_and_takes_no_lock() -> Result<()> {
        let (daft, _server) = run_server(vec![(200, daft_page(&[1]))])?;
        // The rent.ie endpoint is dead, so the detail lookup fails (logged).
        let endpoints = Endpoints {
            daft_ie_api: daft,
            rent_ie: "http://127.0.0.1:1".to_string(),
            ..Endpoints::default()
        };
        let state_dir = tempfile::tempdir()?;

        run(
            Config {
                dry_run: true,
                ..config(Sources {
                    daft_ie_request: Some(DAFT_REQUEST.to_string()),
                    ..Sources::default()
                })
            },
            &endpoints,
            state_dir.path(),
        )?;

        assert_eq!(std::fs::read_dir(state_dir.path())?.count(), 0);
        Ok(())
    }

    #[test]
    fn refuses_to_run_while_another_run_holds_the_lock() -> Result<()> {
        let state_dir = tempfile::tempdir()?;
        let _held = state::lock(&state_dir.path().join(state::LOCK_FILENAME))?;

        let result = run(
            Config {
                webhook_url: Some("http://127.0.0.1:1/".to_string()),
                ..config(Sources {
                    rent_ie_url: Some(DEAD_URL.to_string()),
                    ..Sources::default()
                })
            },
            &Endpoints::default(),
            state_dir.path(),
        );

        assert!(result.is_err_and(|error| error.to_string().contains("in progress")));
        Ok(())
    }

    #[test]
    fn summarizes_a_run_in_one_line() {
        let summary = Summary {
            fetched: vec![("rent.ie", 26), ("daft.ie", 31)],
            unique: 40,
            new: 3,
            outcome: "3 sent".to_string(),
        };
        assert_eq!(
            summary.to_string(),
            "Summary: fetched rent.ie 26, daft.ie 31 → 40 unique, 3 new; 3 sent"
        );
        assert_eq!(
            Summary::default().to_string(),
            "Summary: fetched nothing → 0 unique, 0 new; "
        );
    }
}
