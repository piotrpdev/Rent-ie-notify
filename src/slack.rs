use std::{fmt::Write as _, time::Duration};

use anyhow::{Result, anyhow};
use nanoserde::SerJson;
use reqwest::{StatusCode, blocking::Client, header::RETRY_AFTER};

use crate::http;
use crate::listing::Listing;
use crate::places::{Place, distance_km, format_distance};
use crate::routing::{RouteMap, format_duration};

#[derive(Debug, SerJson)]
struct WebhookPayload {
    blocks: Vec<Block>,
}

#[derive(Debug, SerJson)]
struct Block {
    #[nserde(rename = "type")]
    block_type: String,

    text: Option<TextObject>,

    accessory: Option<Accessory>,
}

#[derive(Debug, SerJson)]
struct TextObject {
    #[nserde(rename = "type")]
    text_type: String,

    text: String,
}

#[derive(Debug, SerJson)]
struct Accessory {
    #[nserde(rename = "type")]
    accessory_type: String,

    image_url: Option<String>,

    alt_text: Option<String>,
}

/// Escapes the characters Slack mrkdwn treats as control sequences.
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Makes a URL safe inside `<url|label>`: `|` would end the URL early and
/// `<`/`>` would end the link.
fn escape_url(url: &str) -> String {
    url.replace('|', "%7C")
        .replace('<', "%3C")
        .replace('>', "%3E")
}

fn distance_line(listing: &Listing, places: &[Place], routes: &RouteMap) -> String {
    let Some(coordinates) = listing.coordinates else {
        return String::new();
    };
    if places.is_empty() {
        return String::new();
    }

    let measurements = routes.get(&listing.id);
    let has_drive = measurements.is_some_and(|route| route.iter().any(Option::is_some));
    let mut entries = Vec::with_capacity(places.len());

    for (index, place) in places.iter().enumerate() {
        let route = measurements
            .and_then(|measurements| measurements.get(index))
            .copied()
            .flatten();

        let entry = route.map_or_else(
            || {
                let distance = format_distance(distance_km(coordinates, place));
                if has_drive {
                    format!("{} {distance} straight-line", escape(&place.name))
                } else {
                    format!("{} {distance}", escape(&place.name))
                }
            },
            |route| {
                format!(
                    "{} {} ({})",
                    escape(&place.name),
                    format_duration(route.duration_seconds),
                    format_distance(route.distance_meters / 1000.0)
                )
            },
        );
        entries.push(entry);
    }

    let label = if has_drive { "*Drive:*" } else { "*Distance:*" };
    format!("\n{label} {}", entries.join(" · "))
}

impl WebhookPayload {
    fn from_listings(
        listings: &[Listing],
        places: &[Place],
        routes: &RouteMap,
        include_images: bool,
    ) -> Self {
        let blocks: Vec<Block> = listings
            .iter()
            .enumerate()
            .flat_map(|(i, listing)| {
                let mut inner_blocks = Vec::new();
                let mut text = format!(
                    "*<{}|{}>*\n{}\n*Price:* {}\n*Posted:* {}",
                    escape_url(&listing.link),
                    escape(&listing.address),
                    escape(&listing.description),
                    escape(&listing.price),
                    escape(&listing.date)
                );

                if !listing.details.available_from.is_empty() {
                    let _ = write!(
                        text,
                        "\n*Available from:* {}",
                        escape(&listing.details.available_from)
                    );
                }
                if !listing.details.minimum_lease.is_empty() {
                    let _ = write!(
                        text,
                        "\n*Minimum lease:* {}",
                        escape(&listing.details.minimum_lease)
                    );
                }
                text.push_str(&distance_line(listing, places, routes));

                let mut listing_block = Block {
                    block_type: "section".to_string(),
                    text: Some(TextObject {
                        text_type: "mrkdwn".to_string(),
                        text,
                    }),
                    accessory: None,
                };

                if include_images && !listing.img_url.is_empty() {
                    listing_block.accessory = Some(Accessory {
                        accessory_type: "image".to_string(),
                        image_url: Some(listing.img_url.clone()),
                        alt_text: Some(listing.address.clone()),
                    });
                }

                inner_blocks.push(listing_block);

                if i < listings.len() - 1 {
                    inner_blocks.push(Block {
                        block_type: "divider".to_string(),
                        text: None,
                        accessory: None,
                    });
                }

                inner_blocks
            })
            .collect();

        Self { blocks }
    }
}

pub(crate) fn listings_to_slack_message_json(
    listings: &[Listing],
    places: &[Place],
    routes: &RouteMap,
    include_images: bool,
) -> String {
    WebhookPayload::from_listings(listings, places, routes, include_images).serialize_json()
}

/// Slack incoming webhooks accept about one message per second.
const MESSAGE_INTERVAL: Duration = Duration::from_millis(1100);
/// How many times a rate-limited (429) message is re-sent.
const MAX_RATE_LIMIT_RETRIES: usize = 3;
/// Upper bound on a single `Retry-After` wait.
const MAX_RETRY_AFTER: Duration = Duration::from_secs(60);

enum PostError {
    /// HTTP 429 with the wait Slack asked for.
    RateLimited(Duration),
    Failed(anyhow::Error),
}

fn post_once(client: &Client, webhook_url: &str, body: &str) -> Result<(), PostError> {
    let response = client
        .post(webhook_url)
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .send()
        .map_err(|error| {
            PostError::Failed(http::without_url(error).context("Failed to send message to Slack"))
        })?;

    let status = response.status();
    if status.is_success() {
        return Ok(());
    }

    if status == StatusCode::TOO_MANY_REQUESTS {
        let retry_after = response
            .headers()
            .get(RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse::<u64>().ok())
            .map_or(MESSAGE_INTERVAL, Duration::from_secs);
        return Err(PostError::RateLimited(retry_after.min(MAX_RETRY_AFTER)));
    }

    // Slack explains rejections in the body, e.g. "invalid_blocks".
    let reason = response.text().unwrap_or_default();
    Err(PostError::Failed(anyhow!(
        "Slack rejected the message with status {}: {}",
        status.as_u16(),
        reason.trim()
    )))
}

/// Posts messages to one webhook, spacing them out to respect Slack's rate
/// limit and waiting out `429 Too Many Requests` responses.
pub(crate) struct Sender {
    client: Client,
    webhook_url: String,
    sleep: fn(Duration),
    has_posted: bool,
}

impl Sender {
    pub(crate) fn new(webhook_url: String) -> Self {
        Self::with_sleep(webhook_url, std::thread::sleep)
    }

    /// Like [`Sender::new`], with a custom sleep (tests pass a no-op).
    pub(crate) fn with_sleep(webhook_url: String, sleep: fn(Duration)) -> Self {
        Self {
            client: Client::new(),
            webhook_url,
            sleep,
            has_posted: false,
        }
    }

    fn post(&mut self, body: &str) -> Result<(), PostError> {
        for attempt in 0..=MAX_RATE_LIMIT_RETRIES {
            if self.has_posted {
                (self.sleep)(MESSAGE_INTERVAL);
            }
            self.has_posted = true;

            match post_once(&self.client, &self.webhook_url, body) {
                Err(PostError::RateLimited(wait)) if attempt < MAX_RATE_LIMIT_RETRIES => {
                    eprintln!(
                        "Slack rate limited the message; retrying in {} s.",
                        wait.as_secs()
                    );
                    (self.sleep)(wait);
                }
                result => return result,
            }
        }

        Err(PostError::Failed(anyhow!(
            "Slack kept rate limiting the message"
        )))
    }

    /// Posts one message for `listings`. Slack rejects the whole message when
    /// it cannot download an image, so a rejected message with images is
    /// retried once without them (rate limiting is handled separately).
    pub(crate) fn send(
        &mut self,
        listings: &[Listing],
        places: &[Place],
        routes: &RouteMap,
    ) -> Result<()> {
        let has_images = listings.iter().any(|listing| !listing.img_url.is_empty());
        let result = self.post(&listings_to_slack_message_json(
            listings, places, routes, true,
        ));

        let result = match result {
            Err(PostError::Failed(error)) if has_images => {
                eprintln!("{error:#}; retrying without images.");
                self.post(&listings_to_slack_message_json(
                    listings, places, routes, false,
                ))
            }
            result => result,
        };

        result.map_err(|error| match error {
            PostError::Failed(error) => error,
            PostError::RateLimited(_) => anyhow!("Slack kept rate limiting the message"),
        })
    }
}

#[cfg(test)]
mod tests {
    use anyhow::{Result, anyhow};

    use super::{Sender, escape, escape_url, listings_to_slack_message_json};
    use crate::listing::{Coordinates, Listing, ListingDetails, ListingSource};
    use crate::places::Place;
    use crate::routing::{RouteMap, RouteMeasurement};
    use crate::test_support::{self, run_sink};

    #[test]
    fn escapes_mrkdwn_control_characters() {
        assert_eq!(
            escape("Smith & Sons <Unit 1>"),
            "Smith &amp; Sons &lt;Unit 1&gt;"
        );
        assert_eq!(
            escape_url("https://example.com/a|b?x=<1>"),
            "https://example.com/a%7Cb?x=%3C1%3E"
        );
    }

    #[test]
    fn includes_configured_distances() {
        let listing = Listing {
            source: ListingSource::Rent,
            link: "https://example.com/1".to_string(),
            coordinates: Some(Coordinates {
                latitude: 52.244_898,
                longitude: -7.159_315,
            }),
            details: ListingDetails {
                available_from: "Now".to_string(),
                minimum_lease: "1 Year".to_string(),
                ..ListingDetails::default()
            },
            ..test_support::listing("1", ListingSource::Rent)
        };
        let places = vec![Place {
            name: "Work".to_string(),
            latitude: 52.244_898,
            longitude: -7.159_315,
        }];

        let message = listings_to_slack_message_json(&[listing], &places, &RouteMap::new(), true);

        assert!(message.contains("*Available from:* Now"));
        assert!(message.contains("*Minimum lease:* 1 Year"));
        assert!(message.contains("*Distance:* Work 0 m"));
    }

    #[test]
    fn includes_drive_times_when_available() {
        let listing = Listing {
            source: ListingSource::Rent,
            link: "https://example.com/1".to_string(),
            coordinates: Some(Coordinates {
                latitude: 52.244_898,
                longitude: -7.159_315,
            }),
            details: ListingDetails {
                available_from: "Now".to_string(),
                minimum_lease: "1 Year".to_string(),
                ..ListingDetails::default()
            },
            ..test_support::listing("1", ListingSource::Rent)
        };
        let places = vec![Place {
            name: "Work".to_string(),
            latitude: 52.244_898,
            longitude: -7.159_315,
        }];
        let mut routes = RouteMap::new();
        routes.insert(
            "1".to_string(),
            vec![Some(RouteMeasurement {
                duration_seconds: 480.0,
                distance_meters: 2400.0,
            })],
        );

        let message = listings_to_slack_message_json(&[listing], &places, &routes, true);

        assert!(message.contains("*Drive:* Work 8 min (2.4 km)"));
    }

    fn listing_with_image() -> Listing {
        Listing {
            img_url: "https://example.com/1.jpg".to_string(),
            ..test_support::listing("1", ListingSource::Daft)
        }
    }

    #[test]
    fn retries_a_rejected_message_without_images() -> Result<()> {
        let (url, sink) = run_sink(vec![400, 200])?;
        Sender::with_sleep(url, |_| {}).send(&[listing_with_image()], &[], &RouteMap::new())?;

        let bodies = sink.join().map_err(|_| anyhow!("sink panicked"))?;
        assert_eq!(bodies.len(), 2);
        assert!(
            bodies
                .first()
                .is_some_and(|body| body.contains("image_url"))
        );
        assert!(
            bodies
                .get(1)
                .is_some_and(|body| !body.contains("image_url"))
        );
        Ok(())
    }

    #[test]
    fn reports_rejections_with_slacks_reason() -> Result<()> {
        let (url, sink) = run_sink(vec![400, 400])?;
        let result =
            Sender::with_sleep(url, |_| {}).send(&[listing_with_image()], &[], &RouteMap::new());

        assert!(result.is_err_and(|error| format!("{error:#}").contains("invalid_blocks")));
        assert_eq!(sink.join().map_err(|_| anyhow!("sink panicked"))?.len(), 2);
        Ok(())
    }

    #[test]
    fn waits_out_rate_limits_and_resends_with_images() -> Result<()> {
        let (url, sink) = run_sink(vec![429, 429, 200])?;

        Sender::with_sleep(url, |_| {}).send(&[listing_with_image()], &[], &RouteMap::new())?;

        let bodies = sink.join().map_err(|_| anyhow!("sink panicked"))?;
        assert_eq!(bodies.len(), 3);
        assert!(bodies.iter().all(|body| body.contains("image_url")));
        Ok(())
    }

    #[test]
    fn gives_up_after_repeated_rate_limits() -> Result<()> {
        let (url, sink) = run_sink(vec![429; 4])?;

        let result =
            Sender::with_sleep(url, |_| {}).send(&[listing_with_image()], &[], &RouteMap::new());

        assert!(result.is_err());
        // 1 attempt + 3 rate-limit retries; no image-less retry, because
        // images weren't the problem.
        assert_eq!(sink.join().map_err(|_| anyhow!("sink panicked"))?.len(), 4);
        Ok(())
    }
}
