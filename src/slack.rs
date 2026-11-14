use nanoserde::{DeJson as Deserialize, SerJson as Serialize};

use crate::listing::Listing;
use crate::places::{Place, distance_km, format_distance};

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

fn distance_line(listing: &Listing, places: &[Place]) -> String {
    let Some(coordinates) = listing.coordinates else {
        return String::new();
    };
    if places.is_empty() {
        return String::new();
    }

    let distances = places
        .iter()
        .map(|place| {
            format!(
                "{} {}",
                place.name,
                format_distance(distance_km(coordinates, place))
            )
        })
        .collect::<Vec<_>>()
        .join(" · ");

    format!("\n*Distance:* {distances}")
}

impl WebhookPayload {
    fn from_listings(listings: &[Listing], places: &[Place]) -> Self {
        let blocks: Vec<Block> = listings
            .iter()
            .enumerate()
            .flat_map(|(i, listing)| {
                let mut inner_blocks = Vec::new();

                let mut listing_block = Block {
                    block_type: "section".to_string(),
                    text: Some(TextObject {
                        text_type: "mrkdwn".to_string(),
                        text: format!(
                            "*<{}|{}>*\n{}\n*Price:* {}\n*Posted:* {}{}",
                            listing.link,
                            listing.address,
                            listing.description,
                            listing.price,
                            listing.date,
                            distance_line(listing, places)
                        ),
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

pub(crate) fn listings_to_slack_message_json(listings: &[Listing], places: &[Place]) -> String {
    let message = WebhookPayload::from_listings(listings, places);
    message.serialize_json()
}

#[cfg(test)]
mod tests {
    use super::listings_to_slack_message_json;
    use crate::listing::{Coordinates, Listing, ListingSource};
    use crate::places::Place;

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
            date: "2026-11-14".to_string(),
            description: "House".to_string(),
        };
        let places = vec![Place {
            name: "Work".to_string(),
            latitude: 52.244_898,
            longitude: -7.159_315,
        }];

        let message = listings_to_slack_message_json(&[listing], &places);

        assert!(message.contains("*Distance:* Work 0 m"));
    }
}
