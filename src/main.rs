use std::{collections::HashSet, env, path::Path, thread};

use crate::listing::Listing;

pub mod daft_ie;
pub mod dedupe;
pub mod delay;
pub mod http;
pub mod listing;
pub mod next_data;
pub mod places;
pub mod property_ie;
pub mod rent_ie;
pub mod routing;
pub mod slack;
pub mod state;
pub mod text;

const DAFT_IE_REQUEST_ENV: &str = "DAFT_IE_REQUEST";
// Slack allows at most 50 Block Kit blocks per message. Each listing uses a
// section plus a divider, so 20 listings is 39 blocks, leaving some headroom.
const SLACK_LISTINGS_PER_MESSAGE: usize = 20;

struct FetchedListings {
    listings: Vec<Listing>,
    rent_build_id: Option<String>,
    /// One message per configured source that failed.
    failures: Vec<String>,
}

/// Joins a source's fetch thread, recording (and logging) a failure instead of
/// propagating it so the other sources can still be processed.
fn join_source<T>(
    name: &str,
    handle: Option<thread::JoinHandle<Result<T, String>>>,
    failures: &mut Vec<String>,
) -> Option<T> {
    let result = handle?
        .join()
        .unwrap_or_else(|_| Err("fetch thread panicked".to_string()));

    match result {
        Ok(value) => Some(value),
        Err(error) => {
            eprintln!("{name} failed, continuing with the other sources: {error}");
            failures.push(format!("{name}: {error}"));
            None
        }
    }
}

/// Fetches all configured sources concurrently. Pagination remains sequential
/// within each source so we don't burst the same host with parallel requests.
/// Fails only if every configured source failed.
fn fetch_all_sources(
    rent_ie_url: Option<String>,
    daft_ie_request: Option<String>,
    property_ie_url: Option<String>,
) -> Result<FetchedListings, String> {
    let rent_thread = rent_ie_url.map(|url| thread::spawn(move || rent_ie::fetch_listings(&url)));
    let daft_thread =
        daft_ie_request.map(|body| thread::spawn(move || daft_ie::fetch_listings(&body)));
    let property_thread =
        property_ie_url.map(|url| thread::spawn(move || property_ie::fetch_listings(&url)));
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
        return Err(format!("All sources failed: {}", failures.join("; ")));
    }

    let (mut listings, rent_build_id) = rent.map_or_else(
        || (Vec::new(), None),
        |(listings, build_id)| (listings, Some(build_id)),
    );
    listings.extend(daft.unwrap_or_default());
    listings.extend(property.unwrap_or_default());

    Ok(FetchedListings {
        listings,
        rent_build_id,
        failures,
    })
}

/// Turns failures that were tolerated during the run (failed sources, Slack
/// rejections) into a non-zero exit, so systemd/journald still flag the run.
fn finish(failures: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if failures.is_empty() {
        println!("Done.");
        Ok(())
    } else {
        Err(format!("Run finished with errors: {}", failures.join("; ")).into())
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

/// Where new listings go.
enum Delivery {
    Slack {
        webhook_url: String,
    },
    /// `DRY_RUN=1`: print the Slack payloads and leave `sent_ids.txt` alone.
    DryRun,
}

/// Posts new listings to Slack in chunks and appends each chunk's group IDs to
/// the state file as soon as Slack accepts it, so a later failure neither loses
/// nor re-sends earlier chunks. Rejected chunks are skipped (and retried next
/// run); failing to record state aborts immediately to avoid duplicates.
fn send_slack_listings(
    delivery: &Delivery,
    listings: &[Listing],
    group_ids: &[Vec<String>],
    places: &[places::Place],
    routes: &routing::RouteMap,
    sent_ids_path: &Path,
) -> Result<(), String> {
    let client = reqwest::blocking::Client::new();
    let mut rejected = 0;

    for (chunk, chunk_ids) in listings
        .chunks(SLACK_LISTINGS_PER_MESSAGE)
        .zip(group_ids.chunks(SLACK_LISTINGS_PER_MESSAGE))
    {
        let webhook_url = match delivery {
            Delivery::Slack { webhook_url } => webhook_url,
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
        if let Err(error) = slack::send(&client, webhook_url, chunk, places, routes) {
            eprintln!("{error}");
            rejected += chunk.len();
            continue;
        }

        state::append(sent_ids_path, &unique_ids(chunk_ids.iter().flatten()))?;
    }

    if rejected > 0 {
        return Err(format!(
            "Slack did not accept {rejected} listings; they will be retried next run"
        ));
    }
    Ok(())
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
        Err(error) => eprintln!("{error}; continuing without a report."),
    }
}

/// Fetches property details for new listings (or for every unique listing
/// with `TEXT_REPORT_INCLUDE_ALL`) and writes the optional text report.
fn prepare_details_and_report(
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

            populate_listing_details(&mut all_listings, rent_build_id);

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
            populate_listing_details(new_listings, rent_build_id);
            if let Some(report_dir) = report_dir {
                write_report(report_dir, new_listings);
            }
        }
    }
}

fn populate_listing_details(listings: &mut [Listing], rent_build_id: Option<String>) {
    if listings.is_empty() {
        return;
    }

    let details_build_id = rent_build_id.map_or_else(
        || match rent_ie::fetch_build_id() {
            Ok(build_id) => Some(build_id),
            Err(error) => {
                eprintln!("Failed to fetch rent.ie build ID for property details: {error}");
                None
            }
        },
        Some,
    );

    if let Some(build_id) = details_build_id {
        rent_ie::add_listing_details(listings, &build_id);
    }
}

/// Reads an environment variable, treating unset and blank values alike so an
/// empty `Environment=RENT_IE_URL=` line disables a source instead of failing it.
fn env_value(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn max_beds_from_env() -> Result<Option<u32>, String> {
    env_value("MAX_BEDS")
        .map(|value| value.parse::<u32>())
        .transpose()
        .map_err(|error| format!("Invalid MAX_BEDS value: {error}"))
}

fn env_flag(name: &str) -> bool {
    env::var(name).is_ok_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes"
        )
    })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dry_run = env_flag("DRY_RUN");
    let seed_only = env_flag("SEED_SENT_IDS");
    let webhook_url = env_value("SLACK_WEBHOOK_URL");
    // Dry runs and seeding never post, so they don't need a webhook.
    if webhook_url.is_none() && !dry_run && !seed_only {
        return Err("Failed to get SLACK_WEBHOOK_URL env var".into());
    }
    if dry_run {
        println!("DRY_RUN is set: Slack payloads are printed and sent_ids.txt is not updated.");
    }
    let rent_ie_url = env_value("RENT_IE_URL");
    let daft_ie_request = env_value(DAFT_IE_REQUEST_ENV);
    let property_ie_url = env_value("PROPERTY_IE_URL");
    let ors_api_key = env_value("OPENROUTESERVICE_API_KEY");
    let text_report_dir = env_value("TEXT_REPORT_DIR");
    let text_report_include_all = env_flag("TEXT_REPORT_INCLUDE_ALL");
    let max_beds = max_beds_from_env()?;

    if rent_ie_url.is_none() && daft_ie_request.is_none() && property_ie_url.is_none() {
        return Err("Set RENT_IE_URL, DAFT_IE_REQUEST, and/or PROPERTY_IE_URL".into());
    }

    let places = match env_value(places::ENV_VAR) {
        Some(value) => places::parse(&value)
            .map_err(|error| format!("Failed to parse {}: {error}", places::ENV_VAR))?,
        None => Vec::new(),
    };

    // Load state before fetching so an unreadable file fails fast.
    println!("Loading already sent listing IDs from file...");
    let sent_ids_path = Path::new(state::SENT_IDS_FILENAME);
    let sent_ids = state::load(sent_ids_path)?;

    let FetchedListings {
        mut listings,
        rent_build_id,
        mut failures,
    } = fetch_all_sources(rent_ie_url, daft_ie_request, property_ie_url)?;

    if let Some(max_beds) = max_beds {
        listings.retain(|listing| listing.bedrooms.is_none_or(|bedrooms| bedrooms <= max_beds));
    }

    let listing_count = listings.len();
    let groups = dedupe::group_listings(listings);
    println!(
        "Grouped {listing_count} listings into {} unique listings.",
        groups.len()
    );

    // Each new group's representative, alongside every ID in that group.
    let (mut new_listings, new_group_ids): (Vec<Listing>, Vec<Vec<String>>) = groups
        .iter()
        .filter(|group| !group.iter().any(|listing| sent_ids.contains(&listing.id)))
        .filter_map(|group| {
            let ids = group.iter().map(|listing| listing.id.clone()).collect();
            Some((dedupe::representative(group)?, ids))
        })
        .unzip();

    if seed_only {
        let ids = unique_ids(new_group_ids.iter().flatten());
        if dry_run {
            println!("DRY_RUN: would seed {} listing IDs.", ids.len());
        } else {
            println!(
                "SEED_SENT_IDS is set: recording {} listing IDs without posting...",
                ids.len()
            );
            state::append(sent_ids_path, &ids)?;
        }
        return finish(&failures);
    }

    prepare_details_and_report(
        &groups,
        &mut new_listings,
        text_report_dir.as_deref(),
        text_report_include_all,
        rent_build_id,
    );

    if new_listings.is_empty() {
        println!("No new listings.");
        return finish(&failures);
    }

    let routes = ors_api_key
        .as_deref()
        .map_or_else(routing::RouteMap::new, |api_key| {
            routing::fetch_routes(&new_listings, &places, api_key)
        });

    let delivery = match webhook_url {
        Some(webhook_url) if !dry_run => Delivery::Slack { webhook_url },
        _ => Delivery::DryRun,
    };
    if let Err(error) = send_slack_listings(
        &delivery,
        &new_listings,
        &new_group_ids,
        &places,
        &routes,
        sent_ids_path,
    ) {
        failures.push(error);
    }

    finish(&failures)
}
