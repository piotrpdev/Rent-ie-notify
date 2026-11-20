use nanoserde::DeJson;
use reqwest::blocking::Client;

use crate::delay;
use crate::http::{self, Page};
use crate::listing::{
    Listing, ListingDetails, ListingSource, bedroom_count, extract_image_keys, format_description,
    format_publish_date,
};
use crate::next_data::{self, Paging};

const RENT_IE_BASE_URL: &str = "https://www.rent.ie";

#[derive(Debug, DeJson)]
struct NextPageData {
    #[nserde(rename = "buildId")]
    build_id: String,
    page: String,
    query: NextQuery,
}

#[derive(Debug, DeJson)]
struct BuildIdData {
    #[nserde(rename = "buildId")]
    build_id: String,
}

#[derive(Debug, DeJson)]
struct NextQuery {
    #[nserde(rename = "propertyType")]
    property_type: String,
    county: String,
    #[nserde(rename = "areaFamily")]
    area_family: Option<String>,
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
    #[nserde(rename = "propertyType")]
    property_type: String,
    paging: Option<Paging>,
}

#[derive(Debug, DeJson)]
struct SearchResult {
    listing: ListingData,
}

#[derive(Debug, DeJson)]
struct PropertyData {
    #[nserde(rename = "pageProps")]
    page_props: PropertyPageProps,
}

#[derive(Debug, DeJson)]
struct PropertyPageProps {
    listing: PropertyListing,
}

#[derive(Debug, DeJson)]
struct PropertyListing {
    #[nserde(rename = "propertyOverview", default)]
    property_overview: Vec<PropertyOverviewItem>,
}

#[derive(Debug, DeJson)]
struct PropertyOverviewItem {
    label: String,
    text: String,
}

fn parse_count(text: &str) -> Option<u32> {
    text.split_whitespace().next()?.parse().ok()
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

fn listing_from_data(listing: ListingData, section: &str) -> Listing {
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

    Listing {
        id: id.to_string(),
        source: ListingSource::Rent,
        link: listing_url(section, &seo_friendly_path, id),
        img_url,
        image_keys,
        coordinates: None,
        address: title.trim().to_string(),
        price: price.trim().to_string(),
        date: publish_date.map(format_publish_date).unwrap_or_default(),
        bedrooms: bedroom_count(num_bedrooms.as_deref()),
        description: format_description([num_bedrooms, num_bathrooms, property_type]),
        details: ListingDetails::default(),
        detail_property_type: section.to_string(),
    }
}

fn listing_url(section: &str, seo_friendly_path: &str, listing_id: u64) -> String {
    // `seoFriendlyPath` is a "/share/<slug>/<id>" path that doesn't resolve on its own;
    // the public listing page is served from "/<section>/<slug>/<id>/".
    let slug = next_data::slug_from_path(seo_friendly_path);

    if slug.is_empty() {
        format!("{RENT_IE_BASE_URL}/{section}/{listing_id}/")
    } else {
        format!("{RENT_IE_BASE_URL}/{section}/{slug}/{listing_id}/")
    }
}

fn parse_next_page_data(html: &str) -> Result<NextPageData, String> {
    let json = next_data::json_from_html(html)?;

    NextPageData::deserialize_json(&json)
        .map_err(|e| format!("Failed to parse __NEXT_DATA__ JSON: {e}"))
}

pub(crate) fn search_api_url(html: &str) -> Result<String, String> {
    let page_data = parse_next_page_data(html)?;

    let route = next_data::data_route(&page_data.page, &page_data.query.rest);

    let mut url = format!(
        "{RENT_IE_BASE_URL}/_next/data/{}/{route}.json?propertyType={}&county={}",
        page_data.build_id, page_data.query.property_type, page_data.query.county
    );

    if let Some(area_family) = &page_data.query.area_family {
        url.push_str("&areaFamily=");
        url.push_str(area_family);
    }

    for segment in &page_data.query.rest {
        url.push_str("&rest=");
        url.push_str(segment);
    }

    Ok(url)
}

pub(crate) fn build_id_from_html(html: &str) -> Result<String, String> {
    let json = next_data::json_from_html(html)?;

    BuildIdData::deserialize_json(&json)
        .map(|data| data.build_id)
        .map_err(|error| format!("Failed to parse __NEXT_DATA__ build ID: {error}"))
}

pub(crate) fn property_details_url(build_id: &str, listing: &Listing) -> Result<String, String> {
    let address = Some(next_data::slug_from_path(&listing.link))
        .filter(|address| !address.is_empty())
        .ok_or_else(|| format!("Could not extract address from {}", listing.link))?;
    let property_type = if listing.detail_property_type.is_empty() {
        "houses-to-let"
    } else {
        &listing.detail_property_type
    };

    Ok(format!(
        "{RENT_IE_BASE_URL}/_next/data/{build_id}/property.json?id={}&propertyType={property_type}&address={address}",
        listing.id
    ))
}

pub(crate) fn extract_details_from_json(json: &str) -> Result<ListingDetails, String> {
    let data: PropertyData = PropertyData::deserialize_json(json)
        .map_err(|error| format!("Failed to parse property JSON: {error}"))?;
    let mut details = ListingDetails::default();

    for item in data.page_props.listing.property_overview {
        let label = item.label.trim();
        if label.eq_ignore_ascii_case("Available From") {
            details.available_from = item.text.trim().to_string();
        } else if label.eq_ignore_ascii_case("Available For")
            || label.eq_ignore_ascii_case("Minimum Lease")
            || label.eq_ignore_ascii_case("Lease Length")
            || label.eq_ignore_ascii_case("Lease")
        {
            details.minimum_lease = item.text.trim().to_string();
        } else if label.eq_ignore_ascii_case("Single Bedroom") {
            details.single_bedrooms = parse_count(item.text.trim());
        } else if label.eq_ignore_ascii_case("Double Bedroom") {
            details.double_bedrooms = parse_count(item.text.trim());
        } else if label.eq_ignore_ascii_case("Twin Bedroom")
            || label.eq_ignore_ascii_case("Twin Room")
        {
            details.twin_bedrooms = parse_count(item.text.trim());
        } else if label.eq_ignore_ascii_case("Bathroom") {
            details.bathrooms = parse_count(item.text.trim());
        }
    }

    Ok(details)
}

pub(crate) fn extract_page_from_json(json: &str) -> Result<Page, String> {
    let data: SearchData =
        SearchData::deserialize_json(json).map_err(|e| format!("Failed to parse API JSON: {e}"))?;
    let SearchPageProps {
        listings,
        property_type,
        paging,
    } = data.page_props;

    let next_page = paging.as_ref().and_then(Paging::next_page);

    let listings = listings
        .into_iter()
        .map(|search_result| listing_from_data(search_result.listing, &property_type))
        .collect();

    Ok((listings, next_page))
}

/// Fetches every results page for a rent.ie search URL. Also returns the
/// Next.js build ID, which is needed for property-detail requests.
pub(crate) fn fetch_listings(search_url: &str) -> Result<(Vec<Listing>, String), String> {
    // `_next/data` URLs include a deployment-specific build ID that is only
    // published in the search page's `__NEXT_DATA__` metadata.
    let client = Client::new();
    println!("Fetching rent.ie search page metadata...");
    let html = http::send_text(client.get(search_url), "rent.ie search page")?;

    let build_id = build_id_from_html(&html)?;
    let api_url =
        search_api_url(&html).map_err(|e| format!("Failed to build rent.ie API URL: {e}"))?;

    // rent.ie ignores pageSize/page_size, so paging is the only way to fetch
    // more than the first 20 results.
    let listings = http::paginate(|page| {
        let page_url =
            page.map_or_else(|| api_url.clone(), |page| format!("{api_url}&page={page}"));
        println!("Fetching rent.ie listings from API...");
        let json = http::send_text(client.get(page_url), "rent.ie listings")?;
        extract_page_from_json(&json)
            .map_err(|e| format!("Failed to extract rent.ie listings from API response: {e}"))
    })?;

    Ok((listings, build_id))
}

/// Reads the current Next.js build ID from the rent.ie homepage.
pub(crate) fn fetch_build_id() -> Result<String, String> {
    let html = http::send_text(Client::new().get(RENT_IE_BASE_URL), "rent.ie homepage")?;
    build_id_from_html(&html)
}

fn fetch_details(
    client: &Client,
    build_id: &str,
    listing: &Listing,
) -> Result<ListingDetails, String> {
    let url = property_details_url(build_id, listing)?;
    let json = http::send_text(
        client.get(url).header("accept", "application/json"),
        "property details",
    )?;
    extract_details_from_json(&json)
}

/// Fills in availability, lease, and room details for each listing. Failures
/// are logged per listing and leave its detail fields empty.
pub(crate) fn add_listing_details(listings: &mut [Listing], build_id: &str) {
    let count = listings.len();
    let client = Client::new();
    println!("Fetching property availability details...");

    for (index, listing) in listings.iter_mut().enumerate() {
        match fetch_details(&client, build_id, listing) {
            Ok(details) => listing.details = details,
            Err(error) => eprintln!(
                "Failed to fetch property details for {}: {error}",
                listing.id
            ),
        }

        if index + 1 < count {
            delay::sleep();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_id_from_html, extract_details_from_json, extract_page_from_json, listing_url,
        search_api_url,
    };

    const PAGE_HTML: &str = r#"<!DOCTYPE html>
<html>
<head>
<script id="__NEXT_DATA__" type="application/json">{"props":{"pageProps":{}},"page":"/search/[[...rest]]","query":{"propertyType":"rooms-to-rent","county":"waterford","areaFamily":"locality","rest":["waterford-city-centre","room-type_either","sort-by_date-entered_down"]},"buildId":"test-build"}</script>
</head>
<body></body>
</html>"#;

    const SEARCH_JSON: &str = r#"{"pageProps":{"propertyType":"rooms-to-rent","listings":[{"listing":{"id":6721798,"title":" Thomas Street, Waterford, Ferrybank, Co. Waterford","price":"€650 per month","numBedrooms":"Double Room","propertyType":"House","media":{"images":[{"size360x240":"https://media.rent.ie/example.jpg"}]},"seoFriendlyPath":"/share/thomas-street-waterford/6721798","publishDate":1794588353317}}]}}"#;

    fn page_html(query: &str) -> String {
        format!(
            r#"<!DOCTYPE html><html><head><script id="__NEXT_DATA__" type="application/json">{{"buildId":"test-build","page":"/search/[[...rest]]","query":{query}}}</script></head><body></body></html>"#
        )
    }

    #[test]
    fn builds_the_search_api_url_from_page_data() -> Result<(), String> {
        assert_eq!(
            search_api_url(PAGE_HTML)?,
            "https://www.rent.ie/_next/data/test-build/search/waterford-city-centre/room-type_either/sort-by_date-entered_down.json?propertyType=rooms-to-rent&county=waterford&areaFamily=locality&rest=waterford-city-centre&rest=room-type_either&rest=sort-by_date-entered_down"
        );

        Ok(())
    }

    #[test]
    fn builds_api_urls_for_houses_and_rooms_searches() -> Result<(), String> {
        let houses_html = page_html(
            r#"{"propertyType":"houses-to-let","county":"waterford","areaFamily":"renting","rest":["waterford-city-centre"]}"#,
        );
        assert_eq!(
            search_api_url(&houses_html)?,
            "https://www.rent.ie/_next/data/test-build/search/waterford-city-centre.json?propertyType=houses-to-let&county=waterford&areaFamily=renting&rest=waterford-city-centre"
        );

        let rooms_html = page_html(
            r#"{"propertyType":"rooms-to-rent","county":"waterford","areaFamily":"renting","rest":["waterford-city-centre","room-type_either"]}"#,
        );
        assert_eq!(
            search_api_url(&rooms_html)?,
            "https://www.rent.ie/_next/data/test-build/search/waterford-city-centre/room-type_either.json?propertyType=rooms-to-rent&county=waterford&areaFamily=renting&rest=waterford-city-centre&rest=room-type_either"
        );

        Ok(())
    }

    #[test]
    fn builds_api_urls_with_max_rent_filters() -> Result<(), String> {
        let houses_html = page_html(
            r#"{"propertyType":"houses-to-let","county":"waterford","areaFamily":"renting","rest":["waterford-city-centre","rent_0-650"]}"#,
        );
        assert_eq!(
            search_api_url(&houses_html)?,
            "https://www.rent.ie/_next/data/test-build/search/waterford-city-centre/rent_0-650.json?propertyType=houses-to-let&county=waterford&areaFamily=renting&rest=waterford-city-centre&rest=rent_0-650"
        );

        let rooms_html = page_html(
            r#"{"propertyType":"rooms-to-rent","county":"waterford","areaFamily":"renting","rest":["waterford-city-centre","rent_0-950"]}"#,
        );
        assert_eq!(
            search_api_url(&rooms_html)?,
            "https://www.rent.ie/_next/data/test-build/search/waterford-city-centre/rent_0-950.json?propertyType=rooms-to-rent&county=waterford&areaFamily=renting&rest=waterford-city-centre&rest=rent_0-950"
        );

        Ok(())
    }

    #[test]
    fn extracts_listing_from_api_json() -> Result<(), String> {
        let (listings, _) = extract_page_from_json(SEARCH_JSON)?;
        assert_eq!(listings.len(), 1);

        let Some(listing) = listings.first() else {
            return Err("expected one listing".to_string());
        };

        assert_eq!(listing.id, "6721798");
        assert_eq!(
            listing.link,
            "https://www.rent.ie/rooms-to-rent/thomas-street-waterford/6721798/"
        );
        assert_eq!(listing.img_url, "https://media.rent.ie/example.jpg");
        assert_eq!(
            listing.address,
            "Thomas Street, Waterford, Ferrybank, Co. Waterford"
        );
        assert_eq!(listing.price, "€650 per month");
        assert_eq!(listing.date, "13th of November, 2026");
        assert_eq!(listing.description, "Double Room · House");

        Ok(())
    }

    #[test]
    fn extracts_build_id_and_property_details() -> Result<(), String> {
        assert_eq!(build_id_from_html(PAGE_HTML)?, "test-build");

        let details = extract_details_from_json(
            r#"{"pageProps":{"listing":{"propertyOverview":[{"label":"Available From","text":"Nov 30th, 2026"},{"label":"Lease","text":"1 Year"},{"label":"Single Bedroom","text":"2"},{"label":"Double Bedroom","text":"1"},{"label":"Twin Bedroom","text":"1"},{"label":"Bathroom","text":"1"}]}}}"#,
        )?;
        assert_eq!(details.available_from, "Nov 30th, 2026");
        assert_eq!(details.minimum_lease, "1 Year");
        assert_eq!(details.single_bedrooms, Some(2));
        assert_eq!(details.double_bedrooms, Some(1));
        assert_eq!(details.twin_bedrooms, Some(1));
        assert_eq!(details.bathrooms, Some(1));

        Ok(())
    }

    #[test]
    fn extracts_the_next_page_from_paging() -> Result<(), String> {
        let json = r#"{"pageProps":{"propertyType":"rooms-to-rent","paging":{"currentPage":2,"totalPages":3},"listings":[]}}"#;
        let (_, next_page) = extract_page_from_json(json)?;
        assert_eq!(next_page, Some(3));

        let json = r#"{"pageProps":{"propertyType":"rooms-to-rent","paging":{"currentPage":3,"totalPages":3},"listings":[]}}"#;
        let (_, next_page) = extract_page_from_json(json)?;
        assert_eq!(next_page, None);

        Ok(())
    }

    #[test]
    fn reports_missing_next_data() {
        assert!(search_api_url("<html><body></body></html>").is_err());
    }

    #[test]
    fn falls_back_to_constructed_listing_url() {
        assert_eq!(
            listing_url(
                "houses-to-let",
                "/share/thomas-street-waterford/6721798",
                6_721_798
            ),
            "https://www.rent.ie/houses-to-let/thomas-street-waterford/6721798/"
        );
    }

    #[test]
    #[ignore = "uses the local archives/ fixtures"]
    fn parses_archived_search_responses() -> Result<(), String> {
        let cases = [
            (
                "archives/rentie/search_waterford_city_centre.json",
                20,
                "6721798",
                "https://www.rent.ie/rooms-to-rent/",
                "Double Room · House",
            ),
            (
                "archives/rentie/search_waterford_city_centre_houses_to_let.json",
                20,
                "6720949",
                "https://www.rent.ie/houses-to-let/",
                "3 Bed · 1 Bath · House",
            ),
            (
                "archives/rentie/search_waterford_city_centre_rooms_to_rent.json",
                20,
                "6721798",
                "https://www.rent.ie/rooms-to-rent/",
                "Double Room · House",
            ),
            (
                "archives/rentie/search_waterford_city_centre_houses_to_let_max_650.json",
                2,
                "6602948",
                "https://www.rent.ie/houses-to-let/",
                "4 Bed · 2 Bath · House",
            ),
            (
                "archives/rentie/search_waterford_city_centre_rooms_to_rent_max_950.json",
                20,
                "6721798",
                "https://www.rent.ie/rooms-to-rent/",
                "Double Room · House",
            ),
        ];

        for (path, expected_count, expected_id, expected_prefix, expected_description) in cases {
            let json =
                std::fs::read_to_string(path).map_err(|e| format!("failed to read {path}: {e}"))?;
            let (listings, _) = extract_page_from_json(&json)?;

            assert_eq!(listings.len(), expected_count, "{path}");
            let Some(first) = listings.first() else {
                return Err(format!("expected listings in {path}"));
            };
            assert_eq!(first.id, expected_id, "{path}");
            assert!(first.link.starts_with(expected_prefix), "{path}");
            assert!(first.link.ends_with(&format!("/{}/", first.id)), "{path}");
            assert_eq!(first.description, expected_description, "{path}");
        }

        let page_two = std::fs::read_to_string(
            "archives/rentie/search_waterford_city_centre_houses_to_let_page_2.json",
        )
        .map_err(|e| format!("failed to read rent.ie page 2 fixture: {e}"))?;
        let (listings, next_page) = extract_page_from_json(&page_two)?;
        assert_eq!(listings.len(), 20);
        assert_eq!(next_page, Some(3));

        Ok(())
    }

    #[test]
    #[ignore = "uses the local archives/ fixtures"]
    fn parses_archived_paginated_search_and_details() -> Result<(), String> {
        let read = |path: &str| {
            std::fs::read_to_string(path).map_err(|e| format!("failed to read {path}: {e}"))
        };
        let (page_one, next_page) = extract_page_from_json(&read(
            "archives/rentie/search_waterford_city_centre_houses_rent_0-1800_page_1.json",
        )?)?;
        assert_eq!(page_one.len(), 20);
        assert_eq!(next_page, Some(2));
        let Some(first) = page_one.first() else {
            return Err("expected listings on page 1".to_string());
        };
        assert_eq!(first.id, "6720949");
        assert_eq!(first.bedrooms, Some(3));
        assert!(first.link.starts_with("https://www.rent.ie/houses-to-let/"));

        let (page_two, next_page) = extract_page_from_json(&read(
            "archives/rentie/search_waterford_city_centre_houses_rent_0-1800_page_2.json",
        )?)?;
        assert_eq!(page_two.len(), 6);
        assert_eq!(next_page, None);

        let details = extract_details_from_json(&read("archives/rentie/property_6721798.json")?)?;
        assert_eq!(details.available_from, "Nov 30th, 2026");
        assert_eq!(details.minimum_lease, "1 Year");

        Ok(())
    }
}
