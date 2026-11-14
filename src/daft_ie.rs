use nanoserde::DeJson;

use crate::listing::{Listing, format_description, format_publish_date};

pub(crate) const API_URL: &str = "https://gateway.daft.ie/api/v2/ads/listings";

const DAFT_IE_BASE_URL: &str = "https://www.daft.ie";

#[derive(Debug, DeJson)]
struct SearchData {
    listings: Vec<SearchResult>,
    paging: Option<Paging>,
}

#[derive(Debug, DeJson)]
struct Paging {
    #[nserde(rename = "nextFrom")]
    next_from: u32,
    #[nserde(rename = "displayingTo")]
    displaying_to: u32,
    #[nserde(rename = "totalResults")]
    total_results: u32,
}

#[derive(Debug, DeJson)]
struct SearchResult {
    listing: ListingData,
}

#[derive(Debug, DeJson)]
struct ListingData {
    id: u64,
    title: String,
    price: String,
    #[nserde(rename = "numBedrooms")]
    num_bedrooms: Option<String>,
    #[nserde(rename = "numBathrooms")]
    num_bathrooms: Option<String>,
    #[nserde(rename = "propertyType")]
    property_type: Option<String>,
    media: Option<ListingMedia>,
    #[nserde(rename = "seoFriendlyPath")]
    seo_friendly_path: String,
    #[nserde(rename = "publishDate")]
    publish_date: Option<u64>,
}

#[derive(Debug, DeJson)]
struct ListingMedia {
    #[nserde(default)]
    images: Vec<ListingImage>,
}

#[derive(Debug, DeJson)]
struct ListingImage {
    #[nserde(rename = "size720x480")]
    large: Option<String>,
    #[nserde(rename = "size72x52")]
    small: Option<String>,
}

fn listing_from_data(listing: ListingData) -> Listing {
    let ListingData {
        id,
        title,
        price,
        num_bedrooms,
        num_bathrooms,
        property_type,
        media,
        seo_friendly_path,
        publish_date,
    } = listing;

    let img_url = media
        .and_then(|media| media.images.into_iter().next())
        .and_then(|image| image.large.or(image.small))
        .unwrap_or_default();

    Listing {
        // Prefix the numeric ID so rent.ie and daft.ie IDs can coexist in sent_ids.txt.
        id: format!("daft-{id}"),
        link: absolute_url(&seo_friendly_path),
        img_url,
        address: title.trim().to_string(),
        price: price.trim().to_string(),
        date: publish_date.map(format_publish_date).unwrap_or_default(),
        description: format_description([num_bedrooms, num_bathrooms, property_type]),
    }
}

fn absolute_url(path: &str) -> String {
    if path.starts_with("http://") || path.starts_with("https://") {
        path.to_string()
    } else if path.starts_with('/') {
        format!("{DAFT_IE_BASE_URL}{path}")
    } else {
        format!("{DAFT_IE_BASE_URL}/{path}")
    }
}

pub(crate) fn extract_page_from_json(json: &str) -> Result<(Vec<Listing>, Option<u32>), String> {
    let data: SearchData = SearchData::deserialize_json(json)
        .map_err(|e| format!("Failed to parse daft.ie API JSON: {e}"))?;

    let next_from = data.paging.and_then(|paging| {
        (paging.next_from > 0 && paging.displaying_to < paging.total_results)
            .then_some(paging.next_from)
    });

    let listings = data
        .listings
        .into_iter()
        .map(|search_result| listing_from_data(search_result.listing))
        .collect();

    Ok((listings, next_from))
}

pub(crate) fn request_body_with_offset(request_body: &str, offset: u32) -> Result<String, String> {
    let paging_start = request_body
        .find("\"paging\"")
        .ok_or("daft.ie request body has no paging field")?;
    let paging = request_body
        .get(paging_start..)
        .ok_or("invalid daft.ie request body")?;
    let from_start = paging
        .find("\"from\"")
        .map(|index| paging_start + index)
        .ok_or("daft.ie request body paging has no from field")?;

    let colon = request_body
        .get(from_start..)
        .and_then(|value| value.find(':'))
        .map(|index| from_start + index + 1)
        .ok_or("invalid daft.ie paging object")?;

    let value_start = request_body
        .get(colon..)
        .and_then(|value| value.find(|character: char| !character.is_whitespace()))
        .map(|index| colon + index)
        .ok_or("missing daft.ie paging offset")?;

    let value = request_body
        .get(value_start..)
        .ok_or("invalid daft.ie paging offset")?;

    let value_len = if value.starts_with('"') {
        value
            .get(1..)
            .and_then(|value| value.find('"'))
            .map(|index| index + 2)
            .ok_or("unterminated daft.ie paging offset")?
    } else {
        value
            .find([',', '}'])
            .ok_or("unterminated daft.ie paging offset")?
    };

    let mut updated = String::with_capacity(request_body.len());
    updated.push_str(
        request_body
            .get(..value_start)
            .ok_or("invalid daft.ie paging offset")?,
    );
    updated.push_str(&offset.to_string());
    updated.push_str(
        request_body
            .get(value_start + value_len..)
            .ok_or("invalid daft.ie paging offset")?,
    );

    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::{extract_page_from_json, request_body_with_offset};

    const SEARCH_JSON: &str = r#"{"listings":[{"listing":{"id":6710820,"title":"Rockshire Road, Ferrybank, Co. Waterford","price":"€1,800 per month","numBedrooms":"2 Bed","numBathrooms":"1 Bath","propertyType":"Apartment","media":{"images":[{"size720x480":"https://media.daft.ie/example.jpg","size72x52":"https://media.daft.ie/thumb.jpg"}]},"seoFriendlyPath":"/for-rent/rockshire-road-ferrybank-co-waterford/6710820","publishDate":1794576513203}}]}"#;

    #[test]
    fn extracts_listing_from_api_json() -> Result<(), String> {
        let (listings, _) = extract_page_from_json(SEARCH_JSON)?;
        assert_eq!(listings.len(), 1);

        let Some(listing) = listings.first() else {
            return Err("expected one listing".to_string());
        };

        assert_eq!(listing.id, "daft-6710820");
        assert_eq!(
            listing.link,
            "https://www.daft.ie/for-rent/rockshire-road-ferrybank-co-waterford/6710820"
        );
        assert_eq!(listing.img_url, "https://media.daft.ie/example.jpg");
        assert_eq!(listing.address, "Rockshire Road, Ferrybank, Co. Waterford");
        assert_eq!(listing.price, "€1,800 per month");
        assert_eq!(listing.date, "2026-11-13");
        assert_eq!(listing.description, "2 Bed · 1 Bath · Apartment");

        Ok(())
    }

    #[test]
    fn updates_the_request_body_paging_offset() -> Result<(), String> {
        let body = r#"{"section":"residential-to-rent","paging":{"from":"0","pageSize":"200"},"terms":""}"#;
        assert_eq!(
            request_body_with_offset(body, 20)?,
            r#"{"section":"residential-to-rent","paging":{"from":20,"pageSize":"200"},"terms":""}"#
        );

        Ok(())
    }

    #[test]
    fn extracts_the_next_offset_from_paging() -> Result<(), String> {
        let json =
            r#"{"listings":[],"paging":{"nextFrom":20,"displayingTo":20,"totalResults":24}}"#;
        assert_eq!(extract_page_from_json(json)?.1, Some(20));

        let json = r#"{"listings":[],"paging":{"nextFrom":0,"displayingTo":24,"totalResults":24}}"#;
        assert_eq!(extract_page_from_json(json)?.1, None);

        Ok(())
    }

    #[test]
    #[ignore = "uses the local archives/ fixture"]
    fn parses_archived_search_response() -> Result<(), String> {
        let json = std::fs::read_to_string("archives/daftie/listings.json")
            .map_err(|e| format!("failed to read archived daft.ie API response: {e}"))?;
        let (listings, _) = extract_page_from_json(&json)?;

        assert_eq!(listings.len(), 20);
        let Some(first) = listings.first() else {
            return Err("expected at least one listing".to_string());
        };
        assert_eq!(first.id, "daft-6710820");
        assert_eq!(
            first.link,
            "https://www.daft.ie/for-rent/rockshire-road-ferrybank-co-waterford/6710820"
        );
        assert_eq!(first.date, "2026-11-13");
        assert_eq!(first.description, "2 Bed · 1 Bath · Apartment");

        let (_, next_from) = extract_page_from_json(&json)?;
        assert_eq!(next_from, Some(20));

        let large_page_json =
            std::fs::read_to_string("archives/daftie/listings_page_size_200.json")
                .map_err(|e| format!("failed to read daft.ie 200-page fixture: {e}"))?;
        let (listings, next_from) = extract_page_from_json(&large_page_json)?;
        assert_eq!(listings.len(), 24);
        assert_eq!(next_from, None);

        Ok(())
    }
}
