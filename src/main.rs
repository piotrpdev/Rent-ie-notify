use std::{env, fs::OpenOptions, io::Write};

use crate::listing::Listing;

pub mod daft_ie;
pub mod listing;
pub mod rent_ie;
pub mod slack;

const SENT_IDS_FILENAME: &str = "sent_ids.txt";
const DAFT_IE_REQUEST_ENV: &str = "DAFT_IE_REQUEST";

fn parse_sent_ids(content: &str) -> Vec<String> {
    // Older versions stored "<slug>/<id>"; rent.ie and daft.ie now expose numeric IDs.
    // Reduce both formats to the final path segment so existing entries keep matching.
    content
        .lines()
        .map(|line| line.trim().rsplit('/').next().unwrap_or_default())
        .filter(|id| !id.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn fetch_rent_ie_listings(rent_ie_url: &str) -> Result<Vec<Listing>, Box<dyn std::error::Error>> {
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
        )
        .into());
    }

    let html_response = response
        .as_str()
        .map_err(|e| format!("Failed to read search page body: {e}"))?;

    let search_api_url = rent_ie::search_api_url(html_response)
        .map_err(|e| format!("Failed to build rent.ie API URL: {e}"))?;

    println!("Fetching rent.ie listings from API...");
    let api_response = minreq::get(search_api_url)
        .send()
        .map_err(|e| format!("Failed to fetch rent.ie listings from API: {e}"))?;

    if api_response.status_code != 200 {
        return Err(format!(
            "Failed to fetch rent.ie listings from API: received status code {}",
            api_response.status_code,
        )
        .into());
    }

    let api_json = api_response
        .as_str()
        .map_err(|e| format!("Failed to read API response body: {e}"))?;

    rent_ie::extract_listings_from_json(api_json)
        .map_err(|e| format!("Failed to extract rent.ie listings from API response: {e}").into())
}

fn fetch_daft_ie_listings(request_body: &str) -> Result<Vec<Listing>, Box<dyn std::error::Error>> {
    println!("Fetching daft.ie listings from API...");
    let response = minreq::post(daft_ie::API_URL)
        .with_header("accept", "application/json")
        .with_header("brand", "daft")
        .with_header("content-type", "application/json")
        .with_header("origin", "https://www.daft.ie")
        .with_header("platform", "web")
        .with_header("referer", "https://www.daft.ie/")
        .with_body(request_body.to_string())
        .send()
        .map_err(|e| format!("Failed to fetch daft.ie listings from API: {e}"))?;

    if response.status_code != 200 {
        return Err(format!(
            "Failed to fetch daft.ie listings from API: received status code {}",
            response.status_code,
        )
        .into());
    }

    let json = response
        .as_str()
        .map_err(|e| format!("Failed to read daft.ie API response body: {e}"))?;

    daft_ie::extract_listings_from_json(json)
        .map_err(|e| format!("Failed to extract daft.ie listings from API response: {e}").into())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let slack_webhook_url =
        env::var("SLACK_WEBHOOK_URL").map_err(|_| "Failed to get SLACK_WEBHOOK_URL env var")?;
    let rent_ie_url = env::var("RENT_IE_URL").ok();
    let daft_ie_request = env::var(DAFT_IE_REQUEST_ENV).ok();

    if rent_ie_url.is_none() && daft_ie_request.is_none() {
        return Err("Set RENT_IE_URL and/or DAFT_IE_REQUEST".into());
    }

    let mut sources: Vec<(&str, Vec<Listing>)> = Vec::new();
    if let Some(rent_ie_url) = rent_ie_url {
        sources.push(("rent.ie", fetch_rent_ie_listings(&rent_ie_url)?));
    }
    if let Some(daft_ie_request) = daft_ie_request {
        sources.push(("daft.ie", fetch_daft_ie_listings(&daft_ie_request)?));
    }

    println!("Loading already sent listing IDs from file...");
    let sent_ids = std::fs::read_to_string(SENT_IDS_FILENAME).map_or_else(
        |_| {
            eprintln!("{SENT_IDS_FILENAME} not found, assuming no listings have been sent yet.");
            Vec::new()
        },
        |content| parse_sent_ids(&content),
    );

    let mut new_ids = Vec::new();

    for (source, listings) in &sources {
        let filtered_listings: Vec<&Listing> = listings
            .iter()
            .filter(|listing| !sent_ids.contains(&listing.id))
            .collect();

        if filtered_listings.is_empty() {
            println!("No new {source} listings.");
            continue;
        }

        println!(
            "Sending {} new {source} listings to Slack...",
            filtered_listings.len()
        );
        let slack_message_json = slack::listings_to_slack_message_json(&filtered_listings);

        minreq::post(&slack_webhook_url)
            .with_header("Content-Type", "application/json")
            .with_body(slack_message_json)
            .send()
            .map_err(|e| format!("Failed to send {source} message to Slack: {e}"))?;

        new_ids.extend(filtered_listings.iter().map(|listing| listing.id.clone()));
    }

    if new_ids.is_empty() {
        println!("Done.");
        return Ok(());
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
        assert_eq!(ids.get(2).map(String::as_str), Some("daft-6710820"));
    }
}
