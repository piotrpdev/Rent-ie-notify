use std::env;

pub mod rent_ie;
pub mod slack;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let rent_url = env::var("RENT_URL").map_err(|_| "Failed to get RENT_URL env var")?;
    let slack_webhook_url =
        env::var("SLACK_WEBHOOK_URL").map_err(|_| "Failed to get SLACK_WEBHOOK_URL env var")?;

    println!("Fetching rent.ie listings...");
    let response = minreq::get(rent_url)
        .send()
        .map_err(|e| format!("Failed to fetch rent.ie listings: {e}"))?;
    let html_response = response
        .as_str()
        .map_err(|e| format!("Failed to read response body: {e}"))?;

    println!("Extracting listings from response HTML...");
    let listings = rent_ie::extract_listings_from_html(html_response);

    println!("Preparing Slack message...");
    let slack_message_json = slack::listings_to_slack_message_json(&listings);

    println!("Sending {} listings to Slack...", listings.len());
    minreq::post(slack_webhook_url)
        .with_header("Content-Type", "application/json")
        .with_body(slack_message_json)
        .send()
        .map_err(|e| format!("Failed to send message to Slack: {e}"))?;

    println!("Done.");
    Ok(())
}
