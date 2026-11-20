use std::fmt::Write as _;

use nanoserde::SerJson;
use reqwest::blocking::Client;

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
                    format!("{} {distance} straight-line", place.name)
                } else {
                    format!("{} {distance}", place.name)
                }
            },
            |route| {
                format!(
                    "{} {} ({})",
                    place.name,
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
                    listing.link, listing.address, listing.description, listing.price, listing.date
                );

                if !listing.details.available_from.is_empty() {
                    let _ = write!(
                        text,
                        "\n*Available from:* {}",
                        listing.details.available_from
                    );
                }
                if !listing.details.minimum_lease.is_empty() {
                    let _ = write!(text, "\n*Minimum lease:* {}", listing.details.minimum_lease);
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

fn post(client: &Client, webhook_url: &str, body: String) -> Result<(), String> {
    let response = client
        .post(webhook_url)
        .header("Content-Type", "application/json")
        .body(body)
        .send()
        .map_err(|error| {
            format!(
                "Failed to send message to Slack: {}",
                http::describe_error(error)
            )
        })?;

    let status = response.status();
    if status.is_success() {
        return Ok(());
    }

    // Slack explains rejections in the body, e.g. "invalid_blocks".
    let reason = response.text().unwrap_or_default();
    Err(format!(
        "Slack rejected the message with status {}: {}",
        status.as_u16(),
        reason.trim()
    ))
}

/// Posts one message for `listings`. Slack rejects the whole message when it
/// cannot download an image, so a rejected message with images is retried once
/// without them.
pub(crate) fn send(
    client: &Client,
    webhook_url: &str,
    listings: &[Listing],
    places: &[Place],
    routes: &RouteMap,
) -> Result<(), String> {
    let has_images = listings.iter().any(|listing| !listing.img_url.is_empty());
    let result = post(
        client,
        webhook_url,
        listings_to_slack_message_json(listings, places, routes, true),
    );

    match result {
        Err(error) if has_images => {
            eprintln!("{error}; retrying without images.");
            post(
                client,
                webhook_url,
                listings_to_slack_message_json(listings, places, routes, false),
            )
        }
        result => result,
    }
}

#[cfg(test)]
mod tests {
    use super::{listings_to_slack_message_json, send};
    use crate::listing::{Coordinates, Listing, ListingDetails, ListingSource};
    use crate::places::Place;
    use crate::routing::{RouteMap, RouteMeasurement};

    #[test]
    fn includes_configured_distances() {
        let listing = Listing {
            id: "1".to_string(),
            source: ListingSource::Rent,
            link: "https://example.com/1".to_string(),
            img_url: String::new(),
            image_keys: Vec::new(),
            coordinates: Some(Coordinates {
                latitude: 52.244_898,
                longitude: -7.159_315,
            }),
            address: "Example address".to_string(),
            price: "€500 per month".to_string(),
            date: "14th of November, 2026".to_string(),
            description: "House".to_string(),
            bedrooms: None,
            details: ListingDetails {
                available_from: "Now".to_string(),
                minimum_lease: "1 Year".to_string(),
                ..ListingDetails::default()
            },
            detail_property_type: "houses-to-let".to_string(),
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
            id: "1".to_string(),
            source: ListingSource::Rent,
            link: "https://example.com/1".to_string(),
            img_url: String::new(),
            image_keys: Vec::new(),
            coordinates: Some(Coordinates {
                latitude: 52.244_898,
                longitude: -7.159_315,
            }),
            address: "Example address".to_string(),
            price: "€500 per month".to_string(),
            date: "14th of November, 2026".to_string(),
            description: "House".to_string(),
            bedrooms: None,
            details: ListingDetails {
                available_from: "Now".to_string(),
                minimum_lease: "1 Year".to_string(),
                ..ListingDetails::default()
            },
            detail_property_type: "houses-to-let".to_string(),
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

    /// Serves one canned status per request and returns the received bodies.
    fn run_sink(
        statuses: Vec<u16>,
    ) -> Result<(String, std::thread::JoinHandle<Vec<String>>), String> {
        let server = tiny_http::Server::http("127.0.0.1:0").map_err(|error| error.to_string())?;
        let port = server
            .server_addr()
            .to_ip()
            .map(|address| address.port())
            .ok_or("sink has no IP address")?;
        let handle = std::thread::spawn(move || {
            let mut bodies = Vec::new();
            for status in statuses {
                let Ok(mut request) = server.recv() else {
                    break;
                };
                let mut body = String::new();
                let _ = request.as_reader().read_to_string(&mut body);
                bodies.push(body);
                let _ = request.respond(
                    tiny_http::Response::from_string("invalid_blocks").with_status_code(status),
                );
            }
            bodies
        });
        Ok((format!("http://127.0.0.1:{port}/"), handle))
    }

    fn listing_with_image() -> Listing {
        Listing {
            id: "1".to_string(),
            source: ListingSource::Daft,
            link: "https://example.com/1".to_string(),
            img_url: "https://example.com/1.jpg".to_string(),
            image_keys: Vec::new(),
            coordinates: None,
            address: "Example address".to_string(),
            price: "€500 per month".to_string(),
            date: String::new(),
            description: "House".to_string(),
            bedrooms: None,
            details: ListingDetails::default(),
            detail_property_type: "houses-to-let".to_string(),
        }
    }

    #[test]
    fn retries_a_rejected_message_without_images() -> Result<(), String> {
        let (url, sink) = run_sink(vec![400, 200])?;
        let client = reqwest::blocking::Client::new();

        send(
            &client,
            &url,
            &[listing_with_image()],
            &[],
            &RouteMap::new(),
        )?;

        let bodies = sink.join().map_err(|_| "sink panicked")?;
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
    fn reports_rejections_with_slacks_reason() -> Result<(), String> {
        let (url, sink) = run_sink(vec![400, 400])?;
        let client = reqwest::blocking::Client::new();

        let result = send(
            &client,
            &url,
            &[listing_with_image()],
            &[],
            &RouteMap::new(),
        );

        assert!(result.is_err_and(|error| error.contains("invalid_blocks")));
        assert_eq!(sink.join().map_err(|_| "sink panicked")?.len(), 2);
        Ok(())
    }
}
