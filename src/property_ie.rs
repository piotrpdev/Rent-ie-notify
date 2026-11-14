use nanoserde::DeJson;
use tl::{Node, ParserOptions};

use crate::listing::{
    Listing, ListingSource, extract_image_keys, format_description, format_publish_date,
};

const NEXT_DATA_SELECTOR: &str = "script#__NEXT_DATA__";
const PROPERTY_IE_BASE_URL: &str = "https://www.property.ie";
pub(crate) const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
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
struct Paging {
    #[nserde(rename = "currentPage")]
    current_page: u32,
    #[nserde(rename = "totalPages")]
    total_pages: u32,
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
        date: publish_date.map(format_publish_date).unwrap_or_default(),
        description: format_description([num_bedrooms, num_bathrooms, property_type]),
        available_from: String::new(),
        minimum_lease: String::new(),
        single_bedrooms: None,
        double_bedrooms: None,
        twin_bedrooms: None,
        bathrooms: None,
        detail_property_type: detail_property_type.to_string(),
    }
}

fn listing_url(ad_type: &str, seo_friendly_path: &str, listing_id: u64) -> String {
    let slug = seo_friendly_path
        .trim_end_matches('/')
        .rsplit('/')
        .nth(1)
        .unwrap_or_default()
        .trim_matches('-');

    format!("{PROPERTY_IE_BASE_URL}/{ad_type}/{slug}/{listing_id}/")
}

fn parse_next_page_data(html: &str) -> Result<NextPageData, String> {
    let dom = tl::parse(html, ParserOptions::default())
        .map_err(|error| format!("Failed to parse HTML: {error}"))?;

    let next_data_script = dom
        .query_selector(NEXT_DATA_SELECTOR)
        .and_then(|mut selector| selector.next())
        .and_then(|node_handle| node_handle.get(dom.parser()))
        .and_then(Node::as_tag)
        .ok_or("Failed to find __NEXT_DATA__ script in HTML")?;

    let json = next_data_script.inner_text(dom.parser());

    NextPageData::deserialize_json(&json)
        .map_err(|error| format!("Failed to parse __NEXT_DATA__ JSON: {error}"))
}

pub(crate) fn search_api_url(html: &str) -> Result<String, String> {
    let page_data = parse_next_page_data(html)?;
    let rest = page_data.query.rest.join("/");
    let route = page_data
        .page
        .replace("[[...rest]]", &rest)
        .trim_matches('/')
        .to_string();
    let route = if route.is_empty() {
        "search".to_string()
    } else {
        route
    };

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

pub(crate) fn extract_page_from_json(json: &str) -> Result<(Vec<Listing>, Option<u32>), String> {
    let data: SearchData = SearchData::deserialize_json(json)
        .map_err(|error| format!("Failed to parse property.ie API JSON: {error}"))?;
    let SearchPageProps {
        listings,
        ad_type,
        paging,
    } = data.page_props;

    let next_page = paging.and_then(|paging| {
        (paging.current_page < paging.total_pages).then_some(paging.current_page + 1)
    });

    let listings = listings
        .into_iter()
        .map(|search_result| listing_from_data(search_result.listing, &ad_type))
        .collect();

    Ok((listings, next_page))
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
}
