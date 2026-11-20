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

/// Turns source failures that were tolerated during the run into a non-zero
/// exit, so systemd/journald still flag the run as failed.
fn finish(failures: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if failures.is_empty() {
        println!("Done.");
        Ok(())
    } else {
        Err(format!("Some sources failed: {}", failures.join("; ")).into())
    }
}

fn send_slack_listings(
    webhook_url: &str,
    listings: &[Listing],
    places: &[crate::places::Place],
    routes: &routing::RouteMap,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::blocking::Client::new();
    for chunk in listings.chunks(SLACK_LISTINGS_PER_MESSAGE) {
        println!("Sending {} new listings to Slack...", chunk.len());
        let slack_message_json = slack::listings_to_slack_message_json(chunk, places, routes);

        client
            .post(webhook_url)
            .header("Content-Type", "application/json")
            .body(slack_message_json)
            .send()
            // The webhook URL is a secret; keep it out of the error message.
            .map_err(|e| {
                format!(
                    "Failed to send message to Slack: {}",
                    http::describe_error(e)
                )
            })?;
    }

    Ok(())
}

fn prepare_details_and_report(
    groups: &[Vec<Listing>],
    new_listings: &mut [Listing],
    report_dir: Option<&str>,
    include_all: bool,
    rent_build_id: Option<String>,
) -> Result<(), String> {
    let Some(report_dir) = report_dir else {
        if !new_listings.is_empty() {
            populate_listing_details(new_listings, rent_build_id);
        }
        return Ok(());
    };

    let report_path = if include_all {
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

        text::write_report(report_dir, &all_listings)?
    } else {
        if !new_listings.is_empty() {
            populate_listing_details(new_listings, rent_build_id);
        }
        text::write_report(report_dir, new_listings)?
    };

    println!("Wrote report to {report_path}");
    Ok(())
}

fn populate_listing_details(listings: &mut [Listing], rent_build_id: Option<String>) {
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

fn max_beds_from_env() -> Result<Option<u32>, String> {
    match env::var("MAX_BEDS") {
        Ok(value) if !value.trim().is_empty() => value
            .trim()
            .parse::<u32>()
            .map(Some)
            .map_err(|error| format!("Invalid MAX_BEDS value: {error}")),
        _ => Ok(None),
    }
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
    let slack_webhook_url =
        env::var("SLACK_WEBHOOK_URL").map_err(|_| "Failed to get SLACK_WEBHOOK_URL env var")?;
    let rent_ie_url = env::var("RENT_IE_URL").ok();
    let daft_ie_request = env::var(DAFT_IE_REQUEST_ENV).ok();
    let property_ie_url = env::var("PROPERTY_IE_URL").ok();
    let ors_api_key = env::var("OPENROUTESERVICE_API_KEY")
        .ok()
        .filter(|value| !value.trim().is_empty());
    let text_report_dir = env::var("TEXT_REPORT_DIR")
        .ok()
        .filter(|value| !value.trim().is_empty());
    let text_report_include_all = env_flag("TEXT_REPORT_INCLUDE_ALL");
    let max_beds = max_beds_from_env()?;

    if rent_ie_url.is_none() && daft_ie_request.is_none() && property_ie_url.is_none() {
        return Err("Set RENT_IE_URL, DAFT_IE_REQUEST, and/or PROPERTY_IE_URL".into());
    }

    let places = match env::var(places::ENV_VAR) {
        Ok(value) => places::parse(&value)
            .map_err(|error| format!("Failed to parse {}: {error}", places::ENV_VAR))?,
        Err(_) => Vec::new(),
    };

    let FetchedListings {
        mut listings,
        rent_build_id,
        failures,
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

    println!("Loading already sent listing IDs from file...");
    let sent_ids_path = Path::new(state::SENT_IDS_FILENAME);
    let sent_ids = state::load(sent_ids_path);

    let new_groups: Vec<Vec<Listing>> = groups
        .iter()
        .filter(|group| !group.iter().any(|listing| sent_ids.contains(&listing.id)))
        .cloned()
        .collect();
    let mut new_listings: Vec<Listing> = new_groups
        .iter()
        .filter_map(|group| dedupe::representative(group))
        .collect();

    prepare_details_and_report(
        &groups,
        &mut new_listings,
        text_report_dir.as_deref(),
        text_report_include_all,
        rent_build_id,
    )?;

    if new_listings.is_empty() {
        println!("No new listings.");
        return finish(&failures);
    }

    let routes = ors_api_key
        .as_deref()
        .map_or_else(routing::RouteMap::new, |api_key| {
            routing::fetch_routes(&new_listings, &places, api_key)
        });

    send_slack_listings(&slack_webhook_url, &new_listings, &places, &routes)?;

    let mut seen = HashSet::new();
    let new_ids: Vec<String> = new_groups
        .iter()
        .flatten()
        .filter(|listing| seen.insert(listing.id.as_str()))
        .map(|listing| listing.id.clone())
        .collect();

    println!("Saving sent listing IDs to {}...", state::SENT_IDS_FILENAME);
    state::append(sent_ids_path, &new_ids)?;

    finish(&failures)
}
