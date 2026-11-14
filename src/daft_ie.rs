use nanoserde::DeJson;

use crate::listing::{Listing, format_description, format_publish_date};

pub(crate) const API_URL: &str = "https://gateway.daft.ie/api/v2/ads/listings";

const DAFT_IE_BASE_URL: &str = "https://www.daft.ie";

#[derive(Debug, DeJson)]
struct SearchData {
    listings: Vec<SearchResult>,
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

pub(crate) fn extract_listings_from_json(json: &str) -> Result<Vec<Listing>, String> {
    let data: SearchData = SearchData::deserialize_json(json)
        .map_err(|e| format!("Failed to parse daft.ie API JSON: {e}"))?;

    Ok(data
        .listings
        .into_iter()
        .map(|search_result| listing_from_data(search_result.listing))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::extract_listings_from_json;

    const SEARCH_JSON: &str = r#"{"listings":[{"listing":{"id":6710820,"title":"Rockshire Road, Ferrybank, Co. Waterford","price":"€1,800 per month","numBedrooms":"2 Bed","numBathrooms":"1 Bath","propertyType":"Apartment","media":{"images":[{"size720x480":"https://media.daft.ie/example.jpg","size72x52":"https://media.daft.ie/thumb.jpg"}]},"seoFriendlyPath":"/for-rent/rockshire-road-ferrybank-co-waterford/6710820","publishDate":1794576513203}}]}"#;

    #[test]
    fn extracts_listing_from_api_json() -> Result<(), String> {
        let listings = extract_listings_from_json(SEARCH_JSON)?;
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
    #[ignore = "uses the local archives/ fixture"]
    fn parses_archived_search_response() -> Result<(), String> {
        let json = std::fs::read_to_string("archives/daftie/listings.json")
            .map_err(|e| format!("failed to read archived daft.ie API response: {e}"))?;
        let listings = extract_listings_from_json(&json)?;

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

        Ok(())
    }
}
