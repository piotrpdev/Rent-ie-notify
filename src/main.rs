use std::{env, fs::OpenOptions, io::Write};

pub mod rent_ie;
pub mod slack;

const SENT_IDS_FILENAME: &str = "sent_ids.txt";

fn parse_sent_ids(content: &str) -> Vec<String> {
    // Older versions stored "<slug>/<id>"; rent.ie's current JSON exposes numeric IDs.
    // Reduce both formats to the numeric ID so existing state entries keep matching.
    content
        .lines()
        .map(|line| line.trim().rsplit('/').next().unwrap_or_default())
        .filter(|id| !id.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let rent_ie_url = env::var("RENT_IE_URL").map_err(|_| "Failed to get RENT_IE_URL env var")?;
    let slack_webhook_url =
        env::var("SLACK_WEBHOOK_URL").map_err(|_| "Failed to get SLACK_WEBHOOK_URL env var")?;

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

    println!("Extracting listings from API response...");
    let listings = rent_ie::extract_listings_from_json(api_json)
        .map_err(|e| format!("Failed to extract listings from API response: {e}"))?;

    println!("Loading already sent listing IDs from file...");
    let sent_ids = std::fs::read_to_string(SENT_IDS_FILENAME).map_or_else(
        |_| {
            eprintln!("{SENT_IDS_FILENAME} not found, assuming no listings have been sent yet.");
            Vec::new()
        },
        |content| parse_sent_ids(&content),
    );

    println!("Filtering out already sent listings...");
    let filtered_listings = listings
        .iter()
        .filter(|listing| !sent_ids.contains(&listing.id))
        .collect();

    println!("Preparing Slack message...");
    let slack_message_json = slack::listings_to_slack_message_json(&filtered_listings);

    println!("Sending {} listings to Slack...", filtered_listings.len());
    minreq::post(slack_webhook_url)
        .with_header("Content-Type", "application/json")
        .with_body(slack_message_json)
        .send()
        .map_err(|e| format!("Failed to send message to Slack: {e}"))?;

    println!("Saving sent listing IDs to {SENT_IDS_FILENAME}...");
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(SENT_IDS_FILENAME)
        .map_err(|e| format!("Failed to open {SENT_IDS_FILENAME}: {e}"))?;

    for listing in &filtered_listings {
        writeln!(file, "{}", listing.id)?;
    }

    println!("Done.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_sent_ids;

    #[test]
    fn parses_legacy_and_current_listing_ids() {
        let ids = parse_sent_ids("2-Belvedere-Drive-Waterford/6010625\n6721798\n\n");

        assert_eq!(ids.len(), 2);
        assert_eq!(ids.first().map(String::as_str), Some("6010625"));
        assert_eq!(ids.get(1).map(String::as_str), Some("6721798"));
    }
}
