use std::{env, fs::OpenOptions, io::Write};

pub mod rent_ie;
pub mod slack;

const SENT_IDS_FILENAME: &str = "sent_ids.txt";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let rent_ie_url = env::var("RENT_IE_URL").map_err(|_| "Failed to get RENT_IE_URL env var")?;
    let slack_webhook_url =
        env::var("SLACK_WEBHOOK_URL").map_err(|_| "Failed to get SLACK_WEBHOOK_URL env var")?;

    println!("Fetching rent.ie listings...");
    let response = minreq::get(rent_ie_url)
        .send()
        .map_err(|e| format!("Failed to fetch rent.ie listings: {e}"))?;

    if response.status_code != 200 {
        return Err(format!(
            "Failed to fetch rent.ie listings: received status code {}. Cloudflare may be blocking the request, try from a residential IP?",
            response.status_code,
        )
        .into());
    }

    let html_response = response
        .as_str()
        .map_err(|e| format!("Failed to read response body: {e}"))?;

    println!("Extracting listings from response HTML...");
    let listings_result = rent_ie::extract_listings_from_html(html_response);

    let Ok(listings) = listings_result else {
        return Err(format!(
            "Failed to extract listings from HTML: {}",
            listings_result.err().unwrap_or_default()
        )
        .into());
    };

    println!("Loading already sent listing IDs from file...");
    let sent_ids = std::fs::read_to_string(SENT_IDS_FILENAME).map_or_else(
        |_| {
            eprintln!("{SENT_IDS_FILENAME} not found, assuming no listings have been sent yet.");
            Vec::new()
        },
        |content| content.lines().map(ToString::to_string).collect(),
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
