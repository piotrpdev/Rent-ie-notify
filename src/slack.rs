use nanoserde::{DeJson as Deserialize, SerJson as Serialize};

use crate::rent_ie::RentListing;

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

impl From<&[RentListing]> for WebhookPayload {
    fn from(listings: &[RentListing]) -> Self {
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
                            "*<{}|{}>*\n{}\n*Price:* {}\n*Available from:* {}",
                            listing.link,
                            listing.address,
                            listing.description,
                            listing.price,
                            listing.date
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

pub(crate) fn listings_to_slack_message_json(listings: &Vec<RentListing>) -> String {
    let message: WebhookPayload = listings.as_slice().into();
    message.serialize_json()
}
