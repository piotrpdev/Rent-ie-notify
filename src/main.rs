use std::{env, fs::OpenOptions, io::Write, thread};

use crate::listing::Listing;

pub mod daft_ie;
pub mod dedupe;
pub mod delay;
pub mod listing;
pub mod places;
pub mod rent_ie;
pub mod routing;
pub mod slack;
pub mod text;

const SENT_IDS_FILENAME: &str = "sent_ids.txt";
const DAFT_IE_REQUEST_ENV: &str = "DAFT_IE_REQUEST";
// Slack allows at most 50 Block Kit blocks per message. Each listing uses a
// section plus a divider, so 20 listings is 39 blocks, leaving some headroom.
const SLACK_LISTINGS_PER_MESSAGE: usize = 20;

fn parse_sent_ids(content: &str) -> Vec<String> {
    // Older versions stored "<slug>/<id>" and later versions stored daft IDs as
    // "daft-<id>". Reduce both formats to the shared numeric ID.
    content
        .lines()
        .map(|line| line.trim().rsplit('/').next().unwrap_or_default())
        .map(|id| id.strip_prefix("daft-").unwrap_or(id))
        .filter(|id| !id.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn fetch_rent_ie_listings(rent_ie_url: &str) -> Result<(Vec<Listing>, String), String> {
    // `_next/data` URLs include a deployment-specific build ID that is only
    // published in the search page's `__NEXT_DATA__` metadata.
    println!("Fetching rent.ie search page metadata...");
    let response = minreq::get(rent_ie_url)
        .send()
        .map_err(|e| format!("Failed to fetch rent.ie search page: {e}"))?;

    if response.status_code != 200 {
        return Err(format!(
            "Failed to fetch rent.ie search page: received status code {}. Cloudflare may be blocking the request, try from a residential IP?",
            response.status_code,
        ));
    }

    let html_response = response
        .as_str()
        .map_err(|e| format!("Failed to read search page body: {e}"))?;

    let build_id = rent_ie::build_id_from_html(html_response)?;
    let search_api_url = rent_ie::search_api_url(html_response)
        .map_err(|e| format!("Failed to build rent.ie API URL: {e}"))?;

    // rent.ie ignores pageSize/page_size, so paging is the only way to fetch
    // more than the first 20 results.
    let mut listings = Vec::new();
    let mut next_page = None;

    loop {
        let page_url = next_page.map_or_else(
            || search_api_url.clone(),
            |page| format!("{search_api_url}&page={page}"),
        );

        println!("Fetching rent.ie listings from API...");
        let api_response = minreq::get(page_url)
            .send()
            .map_err(|e| format!("Failed to fetch rent.ie listings from API: {e}"))?;

        if api_response.status_code != 200 {
            return Err(format!(
                "Failed to fetch rent.ie listings from API: received status code {}",
                api_response.status_code,
            ));
        }

        let api_json = api_response
            .as_str()
            .map_err(|e| format!("Failed to read API response body: {e}"))?;

        let (page_listings, page_next) = rent_ie::extract_page_from_json(api_json)
            .map_err(|e| format!("Failed to extract rent.ie listings from API response: {e}"))?;
        listings.extend(page_listings);

        let Some(page_next) = page_next else {
            break;
        };
        delay::sleep();
        next_page = Some(page_next);
    }

    Ok((listings, build_id))
}

fn fetch_daft_ie_listings(request_body: &str) -> Result<Vec<Listing>, String> {
    let mut listings = Vec::new();
    let mut request_body = request_body.to_string();

    loop {
        println!("Fetching daft.ie listings from API...");
        // The daft gateway is strict about browser-like headers: without
        // brand/platform it returns 400, and without origin/referer it returns 403.
        let response = minreq::post(daft_ie::API_URL)
            .with_header("accept", "application/json")
            .with_header("brand", "daft")
            .with_header("content-type", "application/json")
            .with_header("origin", "https://www.daft.ie")
            .with_header("platform", "web")
            .with_header("referer", "https://www.daft.ie/")
            .with_body(request_body.clone())
            .send()
            .map_err(|e| format!("Failed to fetch daft.ie listings from API: {e}"))?;

        if response.status_code != 200 {
            return Err(format!(
                "Failed to fetch daft.ie listings from API: received status code {}",
                response.status_code,
            ));
        }

        let json = response
            .as_str()
            .map_err(|e| format!("Failed to read daft.ie API response body: {e}"))?;

        let (page_listings, next_from) = daft_ie::extract_page_from_json(json)
            .map_err(|e| format!("Failed to extract daft.ie listings from API response: {e}"))?;
        listings.extend(page_listings);

        let Some(next_from) = next_from else {
            break;
        };

        request_body = daft_ie::request_body_with_offset(&request_body, next_from)
            .map_err(|e| format!("Failed to update daft.ie paging: {e}"))?;
        delay::sleep();
    }

    Ok(listings)
}

fn fetch_rent_build_id() -> Result<String, String> {
    let response = minreq::get("https://www.rent.ie/")
        .send()
        .map_err(|error| format!("Failed to fetch rent.ie homepage: {error}"))?;

    if response.status_code != 200 {
        return Err(format!(
            "Failed to fetch rent.ie homepage: received status code {}",
            response.status_code
        ));
    }

    let html = response
        .as_str()
        .map_err(|error| format!("Failed to read rent.ie homepage: {error}"))?;

    rent_ie::build_id_from_html(html)
}

fn add_listing_details(listings: &mut [Listing], build_id: &str) {
    let count = listings.len();
    println!("Fetching property availability details...");

    for (index, listing) in listings.iter_mut().enumerate() {
        let result = (|| -> Result<(), String> {
            let url = rent_ie::property_details_url(build_id, listing)?;
            let response = minreq::get(url)
                .with_header("accept", "application/json")
                .send()
                .map_err(|error| format!("Failed to fetch property details: {error}"))?;

            if response.status_code != 200 {
                return Err(format!(
                    "property details returned status {}",
                    response.status_code
                ));
            }

            let json = response
                .as_str()
                .map_err(|error| format!("Failed to read property details: {error}"))?;
            let details = rent_ie::extract_details_from_json(json)?;
            listing.available_from = details.available_from;
            listing.minimum_lease = details.minimum_lease;
            listing.single_bedrooms = details.single_bedrooms;
            listing.double_bedrooms = details.double_bedrooms;
            listing.twin_bedrooms = details.twin_bedrooms;
            listing.bathrooms = details.bathrooms;

            Ok(())
        })();

        if let Err(error) = result {
            eprintln!(
                "Failed to fetch property details for {}: {error}",
                listing.id
            );
        }

        if index + 1 < count {
            delay::sleep();
        }
    }
}

fn copy_detail_fields(target: &mut Listing, source: &Listing) {
    target.available_from.clone_from(&source.available_from);
    target.minimum_lease.clone_from(&source.minimum_lease);
    target.single_bedrooms = source.single_bedrooms;
    target.double_bedrooms = source.double_bedrooms;
    target.twin_bedrooms = source.twin_bedrooms;
    target.bathrooms = source.bathrooms;
}

fn send_slack_listings(
    webhook_url: &str,
    listings: &[Listing],
    places: &[crate::places::Place],
    routes: &routing::RouteMap,
) -> Result<(), Box<dyn std::error::Error>> {
    for chunk in listings.chunks(SLACK_LISTINGS_PER_MESSAGE) {
        println!("Sending {} new listings to Slack...", chunk.len());
        let slack_message_json = slack::listings_to_slack_message_json(chunk, places, routes);

        minreq::post(webhook_url)
            .with_header("Content-Type", "application/json")
            .with_body(slack_message_json)
            .send()
            .map_err(|e| format!("Failed to send message to Slack: {e}"))?;
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
                copy_detail_fields(listing, source);
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
        || match fetch_rent_build_id() {
            Ok(build_id) => Some(build_id),
            Err(error) => {
                eprintln!("Failed to fetch rent.ie build ID for property details: {error}");
                None
            }
        },
        Some,
    );

    if let Some(build_id) = details_build_id {
        add_listing_details(listings, &build_id);
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
    let ors_api_key = env::var("OPENROUTESERVICE_API_KEY")
        .ok()
        .filter(|value| !value.trim().is_empty());
    let text_report_dir = env::var("TEXT_REPORT_DIR")
        .ok()
        .filter(|value| !value.trim().is_empty());
    let text_report_include_all = env_flag("TEXT_REPORT_INCLUDE_ALL");

    if rent_ie_url.is_none() && daft_ie_request.is_none() {
        return Err("Set RENT_IE_URL and/or DAFT_IE_REQUEST".into());
    }

    let places = match env::var(places::ENV_VAR) {
        Ok(value) => places::parse(&value)
            .map_err(|error| format!("Failed to parse {}: {error}", places::ENV_VAR))?,
        Err(_) => Vec::new(),
    };

    // Fetch both sources concurrently. Pagination remains sequential within each
    // source so we don't burst the same host with parallel requests.
    let rent_thread = rent_ie_url.map(|url| thread::spawn(move || fetch_rent_ie_listings(&url)));
    let daft_thread =
        daft_ie_request.map(|body| thread::spawn(move || fetch_daft_ie_listings(&body)));

    let rent_result: Result<(Vec<Listing>, Option<String>), String> = match rent_thread {
        Some(handle) => handle
            .join()
            .map_err(|_| "rent.ie fetch thread panicked".to_string())?
            .map(|(listings, build_id)| (listings, Some(build_id))),
        None => Ok((Vec::new(), None)),
    };
    let daft_result = match daft_thread {
        Some(handle) => handle
            .join()
            .map_err(|_| "daft.ie fetch thread panicked".to_string())?,
        None => Ok(Vec::new()),
    };

    let (mut listings, rent_build_id) = rent_result?;
    listings.extend(daft_result?);

    let listing_count = listings.len();
    let groups = dedupe::group_listings(listings);
    println!(
        "Grouped {listing_count} listings into {} unique listings.",
        groups.len()
    );

    println!("Loading already sent listing IDs from file...");
    let sent_ids = std::fs::read_to_string(SENT_IDS_FILENAME).map_or_else(
        |_| {
            eprintln!("{SENT_IDS_FILENAME} not found, assuming no listings have been sent yet.");
            Vec::new()
        },
        |content| parse_sent_ids(&content),
    );

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
        println!("Done.");
        return Ok(());
    }

    let routes = ors_api_key
        .as_deref()
        .map_or_else(routing::RouteMap::new, |api_key| {
            routing::fetch_routes(&new_listings, &places, api_key)
        });

    send_slack_listings(&slack_webhook_url, &new_listings, &places, &routes)?;

    let mut new_ids = Vec::new();
    for group in &new_groups {
        for listing in group {
            if !new_ids.contains(&listing.id) {
                new_ids.push(listing.id.clone());
            }
        }
    }

    println!("Saving sent listing IDs to {SENT_IDS_FILENAME}...");
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(SENT_IDS_FILENAME)
        .map_err(|e| format!("Failed to open {SENT_IDS_FILENAME}: {e}"))?;

    for id in new_ids {
        writeln!(file, "{id}")?;
    }

    println!("Done.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_sent_ids;

    #[test]
    fn parses_legacy_and_current_listing_ids() {
        let ids = parse_sent_ids("2-Belvedere-Drive-Waterford/6010625\n6721798\ndaft-6710820\n\n");

        assert_eq!(ids.len(), 3);
        assert_eq!(ids.first().map(String::as_str), Some("6010625"));
        assert_eq!(ids.get(1).map(String::as_str), Some("6721798"));
        assert_eq!(ids.get(2).map(String::as_str), Some("6710820"));
    }
}
