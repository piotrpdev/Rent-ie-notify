use nanoserde::DeJson;
use reqwest::blocking::Client;

use crate::http::{self, Page};
use crate::listing::{
    Listing, ListingDetails, ListingSource, bedroom_count, extract_image_keys, format_description,
    format_publish_date,
};
use crate::next_data::{self, Paging};

const PROPERTY_IE_BASE_URL: &str = "https://www.property.ie";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
     (KHTML, like Gecko) Chrome/157.0.0.0 Safari/537.36";

#[derive(Debug, DeJson)]
struct NextPageData {
    #[nserde(rename = "buildId")]
    build_id: String,
    page: String,
    query: NextQuery,
}

#[derive(Debug, DeJson)]
struct NextQuery {
    #[nserde(rename = "adType")]
    ad_type: String,
    county: String,
    #[nserde(default)]
    rest: Vec<String>,
}

#[derive(Debug, DeJson)]
struct SearchData {
    #[nserde(rename = "pageProps")]
    page_props: SearchPageProps,
}

#[derive(Debug, DeJson)]
struct SearchPageProps {
    listings: Vec<SearchResult>,
    #[nserde(rename = "adType")]
    ad_type: String,
    #[nserde(default)]
    paging: Option<Paging>,
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
    #[nserde(rename = "size360x240")]
    thumbnail: Option<String>,
}

fn listing_from_data(listing: ListingData, ad_type: &str) -> Listing {
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

    let image_urls: Vec<String> = media
        .into_iter()
        .flat_map(|media| media.images)
        .filter_map(|image| image.thumbnail)
        .collect();
    let img_url = image_urls.first().cloned().unwrap_or_default();
    let image_keys = extract_image_keys(&image_urls);
    let detail_property_type = if ad_type.contains("share") {
        "rooms-to-rent"
    } else {
        "houses-to-let"
    };

    Listing {
        id: id.to_string(),
        source: ListingSource::Property,
        link: listing_url(ad_type, &seo_friendly_path, id),
        img_url,
        image_keys,
        coordinates: None,
        address: title.trim().to_string(),
        price: price.trim().to_string(),
        date: publish_date.map_or_default(format_publish_date),
        bedrooms: bedroom_count(num_bedrooms.as_deref()),
        description: format_description([num_bedrooms, num_bathrooms, property_type]),
        details: ListingDetails::default(),
        detail_property_type: detail_property_type.to_string(),
    }
}

fn listing_url(ad_type: &str, seo_friendly_path: &str, listing_id: u64) -> String {
    let slug = next_data::slug_from_path(seo_friendly_path).trim_matches('-');

    format!("{PROPERTY_IE_BASE_URL}/{ad_type}/{slug}/{listing_id}/")
}

fn parse_next_page_data(html: &str) -> Result<NextPageData, String> {
    let json = next_data::json_from_html(html)?;

    NextPageData::deserialize_json(&json)
        .map_err(|error| format!("Failed to parse __NEXT_DATA__ JSON: {error}"))
}

pub(crate) fn search_api_url(html: &str) -> Result<String, String> {
    let page_data = parse_next_page_data(html)?;
    let route = next_data::data_route(&page_data.page, &page_data.query.rest);

    let mut url = format!(
        "{PROPERTY_IE_BASE_URL}/_next/data/{}/{route}.json?adType={}&county={}",
        page_data.build_id, page_data.query.ad_type, page_data.query.county
    );

    for segment in &page_data.query.rest {
        url.push_str("&rest=");
        url.push_str(segment);
    }

    Ok(url)
}

pub(crate) fn extract_page_from_json(json: &str) -> Result<Page, String> {
    let data: SearchData = SearchData::deserialize_json(json)
        .map_err(|error| format!("Failed to parse property.ie API JSON: {error}"))?;
    let SearchPageProps {
        listings,
        ad_type,
        paging,
    } = data.page_props;

    let next_page = paging.as_ref().and_then(Paging::next_page);

    let listings = listings
        .into_iter()
        .map(|search_result| listing_from_data(search_result.listing, &ad_type))
        .collect();

    Ok((listings, next_page))
}

/// Fetches every results page for a property.ie search URL.
pub(crate) fn fetch_listings(search_url: &str) -> Result<Vec<Listing>, String> {
    // property.ie requires HTTP/2; Cloudflare returns 403 for HTTP/1.1 requests.
    let client = Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .map_err(|error| format!("Failed to create property.ie HTTP client: {error}"))?;

    println!("Fetching property.ie search page metadata...");
    let request = client
        .get(search_url)
        .header(
            "accept",
            "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8",
        )
        .header("accept-language", "en-GB,en-US;q=0.9,en;q=0.8")
        .header("referer", "https://www.property.ie/");
    let html = http::send_text(request, "property.ie search page")?;
    let api_url = search_api_url(&html)?;

    http::paginate(|page| {
        let page_url =
            page.map_or_else(|| api_url.clone(), |page| format!("{api_url}&page={page}"));
        println!("Fetching property.ie listings from API...");
        let request = client.get(page_url).header("accept", "application/json");
        let json = http::send_text(request, "property.ie listings")?;
        extract_page_from_json(&json)
    })
}

#[cfg(test)]
mod tests {
    use super::{extract_page_from_json, listing_url, search_api_url};

    const PAGE_HTML: &str = r#"<!DOCTYPE html>
<html>
<head>
<script id="__NEXT_DATA__" type="application/json">{"props":{},"page":"/search/[[...rest]]","query":{"adType":"property-to-let","county":"waterford","rest":["price_0-3500","beds_4"]},"buildId":"test-build"}</script>
</head>
<body></body>
</html>"#;

    const SEARCH_JSON: &str = r#"{"pageProps":{"adType":"property-to-let","paging":{"currentPage":1,"totalPages":2},"listings":[{"listing":{"id":6721266,"title":"Lakunda, West Street, Tallow, Co. Waterford","price":"€1,600 per month","numBedrooms":"4 Bed","numBathrooms":"1 Bath","propertyType":"House","media":{"images":[{"size360x240":"https://media.property.ie/example.jpg"}]},"seoFriendlyPath":"/for-rent/lakunda-west-street-tallow-co-waterford/6721266","publishDate":1794564618513}}]}}"#;

    #[test]
    fn builds_the_property_ie_search_api_url() -> Result<(), String> {
        assert_eq!(
            search_api_url(PAGE_HTML)?,
            "https://www.property.ie/_next/data/test-build/search/price_0-3500/beds_4.json?adType=property-to-let&county=waterford&rest=price_0-3500&rest=beds_4"
        );
        Ok(())
    }

    #[test]
    fn builds_public_property_urls() {
        assert_eq!(
            listing_url(
                "property-to-let",
                "/for-rent/-barrack-street-waterford-waterford-city-centre-co-waterford/6719091",
                6_719_091
            ),
            "https://www.property.ie/property-to-let/barrack-street-waterford-waterford-city-centre-co-waterford/6719091/"
        );
    }

    #[test]
    fn extracts_a_property_ie_listing() -> Result<(), String> {
        let (listings, next_page) = extract_page_from_json(SEARCH_JSON)?;
        assert_eq!(listings.len(), 1);
        assert_eq!(next_page, Some(2));

        let Some(listing) = listings.first() else {
            return Err("expected a listing".to_string());
        };
        assert_eq!(listing.id, "6721266");
        assert_eq!(
            listing.link,
            "https://www.property.ie/property-to-let/lakunda-west-street-tallow-co-waterford/6721266/"
        );
        assert_eq!(
            listing.address,
            "Lakunda, West Street, Tallow, Co. Waterford"
        );
        assert_eq!(listing.price, "€1,600 per month");
        assert_eq!(listing.date, "13th of November, 2026");
        assert_eq!(listing.detail_property_type, "houses-to-let");

        Ok(())
    }

    #[test]
    #[ignore = "uses the local archives/ fixtures"]
    fn parses_archived_search_and_details() -> Result<(), String> {
        let read = |path: &str| {
            std::fs::read_to_string(path).map_err(|e| format!("failed to read {path}: {e}"))
        };
        let (listings, next_page) = extract_page_from_json(&read(
            "archives/propertyie/search_waterford_4beds_0-3500_page_1.json",
        )?)?;
        assert_eq!(listings.len(), 18);
        assert_eq!(next_page, None);
        let Some(first) = listings.first() else {
            return Err("expected listings".to_string());
        };
        assert_eq!(first.id, "6721266");
        assert_eq!(first.bedrooms, Some(4));
        assert_eq!(
            first.link,
            "https://www.property.ie/property-to-let/lakunda-west-street-tallow-co-waterford/6721266/"
        );
        assert!(listings.iter().all(|listing| {
            listing
                .link
                .starts_with("https://www.property.ie/property-to-let/")
        }));

        // This property response has no propertyOverview; details stay empty
        // rather than failing.
        let details = crate::rent_ie::extract_details_from_json(&read(
            "archives/propertyie/property_6721266.json",
        )?)?;
        assert_eq!(details.available_from, "");
        assert_eq!(details.bathrooms, None);

        Ok(())
    }
}
