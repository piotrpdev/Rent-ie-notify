use std::{env, fs::OpenOptions, io::Write, thread, time::Duration};

use crate::listing::Listing;

pub mod daft_ie;
pub mod dedupe;
pub mod listing;
pub mod rent_ie;
pub mod slack;

const SENT_IDS_FILENAME: &str = "sent_ids.txt";
const DAFT_IE_REQUEST_ENV: &str = "DAFT_IE_REQUEST";
// Slack allows at most 50 Block Kit blocks per message. Each listing uses a
// section plus a divider, so 20 listings is 39 blocks, leaving some headroom.
const SLACK_LISTINGS_PER_MESSAGE: usize = 20;
// Pause between paginated API calls so we don't trigger rate limits.
const PAGE_REQUEST_DELAY: Duration = Duration::from_secs(1);

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

fn fetch_rent_ie_listings(rent_ie_url: &str) -> Result<Vec<Listing>, String> {
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
        thread::sleep(PAGE_REQUEST_DELAY);
        next_page = Some(page_next);
    }

    Ok(listings)
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
        thread::sleep(PAGE_REQUEST_DELAY);
    }

    Ok(listings)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let slack_webhook_url =
        env::var("SLACK_WEBHOOK_URL").map_err(|_| "Failed to get SLACK_WEBHOOK_URL env var")?;
    let rent_ie_url = env::var("RENT_IE_URL").ok();
    let daft_ie_request = env::var(DAFT_IE_REQUEST_ENV).ok();

    if rent_ie_url.is_none() && daft_ie_request.is_none() {
        return Err("Set RENT_IE_URL and/or DAFT_IE_REQUEST".into());
    }

    // Fetch both sources concurrently. Pagination remains sequential within each
    // source so we don't burst the same host with parallel requests.
    let rent_thread = rent_ie_url.map(|url| thread::spawn(move || fetch_rent_ie_listings(&url)));
    let daft_thread =
        daft_ie_request.map(|body| thread::spawn(move || fetch_daft_ie_listings(&body)));

    let rent_result = match rent_thread {
        Some(handle) => handle
            .join()
            .map_err(|_| "rent.ie fetch thread panicked".to_string())?,
        None => Ok(Vec::new()),
    };
    let daft_result = match daft_thread {
        Some(handle) => handle
            .join()
            .map_err(|_| "daft.ie fetch thread panicked".to_string())?,
        None => Ok(Vec::new()),
    };

    let mut listings = rent_result?;
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
        .into_iter()
        .filter(|group| !group.iter().any(|listing| sent_ids.contains(&listing.id)))
        .collect();
    let new_listings: Vec<&Listing> = new_groups
        .iter()
        .filter_map(|group| group.first())
        .collect();

    if new_listings.is_empty() {
        println!("No new listings.");
        println!("Done.");
        return Ok(());
    }

    for chunk in new_listings.chunks(SLACK_LISTINGS_PER_MESSAGE) {
        println!("Sending {} new listings to Slack...", chunk.len());
        let slack_message_json = slack::listings_to_slack_message_json(chunk);

        minreq::post(&slack_webhook_url)
            .with_header("Content-Type", "application/json")
            .with_body(slack_message_json)
            .send()
            .map_err(|e| format!("Failed to send message to Slack: {e}"))?;
    }

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
