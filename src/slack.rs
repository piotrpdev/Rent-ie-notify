use std::fmt::Write as _;

use nanoserde::{DeJson as Deserialize, SerJson as Serialize};

use crate::listing::Listing;
use crate::places::{Place, distance_km, format_distance};
use crate::routing::{RouteMap, format_duration};

#[derive(Debug, Serialize, Deserialize)]
pub struct WebhookPayload {
    pub blocks: Vec<Block>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Block {
    #[nserde(rename = "type")]
    pub block_type: String,

    #[nserde(default)]
    pub text: Option<TextObject>,

    #[nserde(default)]
    pub accessory: Option<Accessory>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TextObject {
    #[nserde(rename = "type")]
    pub text_type: String,

    pub text: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Accessory {
    #[nserde(rename = "type")]
    pub accessory_type: String,

    #[nserde(default)]
    pub image_url: Option<String>,

    #[nserde(default)]
    pub alt_text: Option<String>,
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
    fn from_listings(listings: &[Listing], places: &[Place], routes: &RouteMap) -> Self {
        let blocks: Vec<Block> = listings
            .iter()
            .enumerate()
            .flat_map(|(i, listing)| {
                let mut inner_blocks = Vec::new();
                let mut text = format!(
                    "*<{}|{}>*\n{}\n*Price:* {}\n*Posted:* {}",
                    listing.link, listing.address, listing.description, listing.price, listing.date
                );

                if !listing.available_from.is_empty() {
                    let _ = write!(text, "\n*Available from:* {}", listing.available_from);
                }
                if !listing.minimum_lease.is_empty() {
                    let _ = write!(text, "\n*Minimum lease:* {}", listing.minimum_lease);
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

                if !listing.img_url.is_empty() {
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
) -> String {
    let message = WebhookPayload::from_listings(listings, places, routes);
    message.serialize_json()
}

#[cfg(test)]
mod tests {
    use super::listings_to_slack_message_json;
    use crate::listing::{Coordinates, Listing, ListingSource};
    use crate::places::Place;
    use crate::routing::{RouteMap, RouteMeasurement};

    #[test]
    fn includes_configured_distances() {
        let listing = Listing {
            id: "1".to_string(),
            source: ListingSource::RentIe,
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
            available_from: "Now".to_string(),
            minimum_lease: "1 Year".to_string(),
            single_bedrooms: None,
            double_bedrooms: None,
            twin_bedrooms: None,
            bathrooms: None,
            detail_property_type: "houses-to-let".to_string(),
        };
        let places = vec![Place {
            name: "Work".to_string(),
            latitude: 52.244_898,
            longitude: -7.159_315,
        }];

        let message = listings_to_slack_message_json(&[listing], &places, &RouteMap::new());

        assert!(message.contains("*Available from:* Now"));
        assert!(message.contains("*Minimum lease:* 1 Year"));
        assert!(message.contains("*Distance:* Work 0 m"));
    }

    #[test]
    fn includes_drive_times_when_available() {
        let listing = Listing {
            id: "1".to_string(),
            source: ListingSource::RentIe,
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
            available_from: "Now".to_string(),
            minimum_lease: "1 Year".to_string(),
            single_bedrooms: None,
            double_bedrooms: None,
            twin_bedrooms: None,
            bathrooms: None,
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

        let message = listings_to_slack_message_json(&[listing], &places, &routes);

        assert!(message.contains("*Drive:* Work 8 min (2.4 km)"));
    }
}
