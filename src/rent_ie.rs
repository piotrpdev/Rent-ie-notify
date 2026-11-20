use anyhow::{Context as _, Result};
use nanoserde::DeJson;
use reqwest::blocking::Client;

use crate::delay;
use crate::http::{self, Page};
use crate::listing::{Listing, ListingDetails, ListingSource, SearchResult, Section};
use crate::next_data::{self, Paging};

/// Public site, used for listing links.
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

fn parse_next_page_data(html: &str) -> Result<NextPageData> {
    let json = next_data::json_from_html(html)?;

    NextPageData::deserialize_json(&json).context("Failed to parse __NEXT_DATA__ JSON")
}

pub(crate) fn search_api_url(base_url: &str, html: &str) -> Result<String> {
    let page_data = parse_next_page_data(html)?;

    let route = next_data::data_route(&page_data.page, &page_data.query.rest);

    let mut url = format!(
        "{base_url}/_next/data/{}/{route}.json?propertyType={}&county={}",
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

pub(crate) fn build_id_from_html(html: &str) -> Result<String> {
    let json = next_data::json_from_html(html)?;

    BuildIdData::deserialize_json(&json)
        .map(|data| data.build_id)
        .context("Failed to parse __NEXT_DATA__ build ID")
}

pub(crate) fn property_details_url(
    base_url: &str,
    build_id: &str,
    listing: &Listing,
) -> Result<String> {
    let address = Some(next_data::slug_from_path(&listing.link))
        .filter(|address| !address.is_empty())
        .with_context(|| format!("Could not extract address from {}", listing.link))?;
    let property_type = listing.section.slug();

    Ok(format!(
        "{base_url}/_next/data/{build_id}/property.json?id={}&propertyType={property_type}&address={address}",
        listing.id
    ))
}

pub(crate) fn extract_details_from_json(json: &str) -> Result<ListingDetails> {
    let data: PropertyData =
        PropertyData::deserialize_json(json).context("Failed to parse property JSON")?;
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

pub(crate) fn extract_page_from_json(json: &str) -> Result<Page> {
    let data: SearchData =
        SearchData::deserialize_json(json).context("Failed to parse API JSON")?;
    let SearchPageProps {
        listings,
        property_type,
        paging,
    } = data.page_props;

    let next_page = paging.as_ref().and_then(Paging::next_page);

    let listings = listings
        .into_iter()
        .map(|SearchResult { listing }| {
            let link = listing_url(&property_type, &listing.seo_friendly_path, listing.id());
            listing.into_listing(
                ListingSource::Rent,
                link,
                Section::from_slug(&property_type),
            )
        })
        .collect();

    Ok((listings, next_page))
}

/// Fetches every results page for a rent.ie search URL. Also returns the
/// Next.js build ID, which is needed for property-detail requests.
pub(crate) fn fetch_listings(base_url: &str, search_url: &str) -> Result<(Vec<Listing>, String)> {
    // `_next/data` URLs include a deployment-specific build ID that is only
    // published in the search page's `__NEXT_DATA__` metadata.
    let client = Client::new();
    println!("Fetching rent.ie search page metadata...");
    let html = http::send_text(client.get(search_url), "rent.ie search page")?;

    let build_id = build_id_from_html(&html)?;
    let api_url = search_api_url(base_url, &html).context("Failed to build rent.ie API URL")?;

    // rent.ie ignores pageSize/page_size, so paging is the only way to fetch
    // more than the first 20 results.
    let listings = http::paginate(|page| {
        let page_url =
            page.map_or_else(|| api_url.clone(), |page| format!("{api_url}&page={page}"));
        println!("Fetching rent.ie listings from API...");
        let json = http::send_text(client.get(page_url), "rent.ie listings")?;
        extract_page_from_json(&json)
            .context("Failed to extract rent.ie listings from API response")
    })?;

    Ok((listings, build_id))
}

/// Reads the current Next.js build ID from the rent.ie homepage.
pub(crate) fn fetch_build_id(base_url: &str) -> Result<String> {
    let html = http::send_text(Client::new().get(base_url), "rent.ie homepage")?;
    build_id_from_html(&html)
}

fn fetch_details(
    client: &Client,
    base_url: &str,
    build_id: &str,
    listing: &Listing,
) -> Result<ListingDetails> {
    let url = property_details_url(base_url, build_id, listing)?;
    let json = http::send_text(
        client.get(url).header("accept", "application/json"),
        "property details",
    )?;
    extract_details_from_json(&json)
}

/// Fills in availability, lease, and room details for each listing. Failures
/// are logged per listing and leave its detail fields empty.
pub(crate) fn add_listing_details(base_url: &str, listings: &mut [Listing], build_id: &str) {
    let count = listings.len();
    let client = Client::new();
    println!("Fetching property availability details...");

    for (index, listing) in listings.iter_mut().enumerate() {
        match fetch_details(&client, base_url, build_id, listing) {
            Ok(details) => listing.details = details,
            Err(error) => eprintln!(
                "Failed to fetch property details for {}: {error:#}",
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
    use anyhow::{Context as _, Result, anyhow, bail};

    use super::{
        add_listing_details, build_id_from_html, extract_details_from_json, extract_page_from_json,
        fetch_listings, listing_url, search_api_url,
    };
    use crate::listing::{Listing, ListingSource};
    const BASE: &str = "https://www.rent.ie";

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
    fn builds_the_search_api_url_from_page_data() -> Result<()> {
        assert_eq!(
            search_api_url(BASE, PAGE_HTML)?,
            "https://www.rent.ie/_next/data/test-build/search/waterford-city-centre/room-type_either/sort-by_date-entered_down.json?propertyType=rooms-to-rent&county=waterford&areaFamily=locality&rest=waterford-city-centre&rest=room-type_either&rest=sort-by_date-entered_down"
        );

        Ok(())
    }

    #[test]
    fn builds_api_urls_for_houses_and_rooms_searches() -> Result<()> {
        let houses_html = page_html(
            r#"{"propertyType":"houses-to-let","county":"waterford","areaFamily":"renting","rest":["waterford-city-centre"]}"#,
        );
        assert_eq!(
            search_api_url(BASE, &houses_html)?,
            "https://www.rent.ie/_next/data/test-build/search/waterford-city-centre.json?propertyType=houses-to-let&county=waterford&areaFamily=renting&rest=waterford-city-centre"
        );

        let rooms_html = page_html(
            r#"{"propertyType":"rooms-to-rent","county":"waterford","areaFamily":"renting","rest":["waterford-city-centre","room-type_either"]}"#,
        );
        assert_eq!(
            search_api_url(BASE, &rooms_html)?,
            "https://www.rent.ie/_next/data/test-build/search/waterford-city-centre/room-type_either.json?propertyType=rooms-to-rent&county=waterford&areaFamily=renting&rest=waterford-city-centre&rest=room-type_either"
        );

        Ok(())
    }

    #[test]
    fn builds_api_urls_with_max_rent_filters() -> Result<()> {
        let houses_html = page_html(
            r#"{"propertyType":"houses-to-let","county":"waterford","areaFamily":"renting","rest":["waterford-city-centre","rent_0-650"]}"#,
        );
        assert_eq!(
            search_api_url(BASE, &houses_html)?,
            "https://www.rent.ie/_next/data/test-build/search/waterford-city-centre/rent_0-650.json?propertyType=houses-to-let&county=waterford&areaFamily=renting&rest=waterford-city-centre&rest=rent_0-650"
        );

        let rooms_html = page_html(
            r#"{"propertyType":"rooms-to-rent","county":"waterford","areaFamily":"renting","rest":["waterford-city-centre","rent_0-950"]}"#,
        );
        assert_eq!(
            search_api_url(BASE, &rooms_html)?,
            "https://www.rent.ie/_next/data/test-build/search/waterford-city-centre/rent_0-950.json?propertyType=rooms-to-rent&county=waterford&areaFamily=renting&rest=waterford-city-centre&rest=rent_0-950"
        );

        Ok(())
    }

    #[test]
    fn extracts_listing_from_api_json() -> Result<()> {
        let (listings, _) = extract_page_from_json(SEARCH_JSON)?;
        assert_eq!(listings.len(), 1);

        let Some(listing) = listings.first() else {
            bail!("expected one listing");
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
    fn extracts_build_id_and_property_details() -> Result<()> {
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
    fn extracts_the_next_page_from_paging() -> Result<()> {
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
        assert!(search_api_url(BASE, "<html><body></body></html>").is_err());
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
    fn parses_archived_search_responses() -> Result<()> {
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
                std::fs::read_to_string(path).with_context(|| format!("failed to read {path}"))?;
            let (listings, _) = extract_page_from_json(&json)?;

            assert_eq!(listings.len(), expected_count, "{path}");
            let Some(first) = listings.first() else {
                bail!("expected listings in {path}");
            };
            assert_eq!(first.id, expected_id, "{path}");
            assert!(first.link.starts_with(expected_prefix), "{path}");
            assert!(first.link.ends_with(&format!("/{}/", first.id)), "{path}");
            assert_eq!(first.description, expected_description, "{path}");
        }

        let page_two = std::fs::read_to_string(
            "archives/rentie/search_waterford_city_centre_houses_to_let_page_2.json",
        )
        .context("failed to read rent.ie page 2 fixture")?;
        let (listings, next_page) = extract_page_from_json(&page_two)?;
        assert_eq!(listings.len(), 20);
        assert_eq!(next_page, Some(3));

        Ok(())
    }

    #[test]
    #[ignore = "uses the local archives/ fixtures"]
    fn parses_archived_paginated_search_and_details() -> Result<()> {
        let read = |path: &str| {
            std::fs::read_to_string(path).with_context(|| format!("failed to read {path}"))
        };
        let (page_one, next_page) = extract_page_from_json(&read(
            "archives/rentie/search_waterford_city_centre_houses_rent_0-1800_page_1.json",
        )?)?;
        assert_eq!(page_one.len(), 20);
        assert_eq!(next_page, Some(2));
        let Some(first) = page_one.first() else {
            bail!("expected listings on page 1");
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

    fn api_page(current: u32, total: u32, id: u32) -> String {
        format!(
            r#"{{"pageProps":{{"propertyType":"houses-to-let","paging":{{"currentPage":{current},"totalPages":{total}}},"listings":[{{"listing":{{"id":{id},"title":"Listing {id}","price":"€1,000 per month","numBedrooms":"2 Bed","seoFriendlyPath":"/share/slug-{id}/{id}"}}}}]}}}}"#
        )
    }

    #[test]
    fn fetches_the_search_page_then_every_api_page() -> Result<()> {
        let (base, server) = crate::test_support::run_server(vec![
            (200, PAGE_HTML.to_string()),
            (200, api_page(1, 2, 1)),
            (200, api_page(2, 2, 2)),
        ])?;

        let (listings, build_id) = fetch_listings(&base, &format!("{base}/search-page"))?;

        assert_eq!(build_id, "test-build");
        let ids: Vec<&str> = listings.iter().map(|listing| listing.id.as_str()).collect();
        assert_eq!(ids, vec!["1", "2"]);
        assert_eq!(
            listings.first().map(|listing| listing.link.as_str()),
            Some("https://www.rent.ie/houses-to-let/slug-1/1/")
        );

        let requests = server.join().map_err(|_| anyhow!("server panicked"))?;
        let urls: Vec<&str> = requests
            .iter()
            .map(|request| request.url.as_str())
            .collect();
        assert_eq!(urls.first(), Some(&"/search-page"));
        assert!(urls.get(1).is_some_and(|url| {
            url.starts_with("/_next/data/test-build/search/") && !url.contains("&page=")
        }));
        assert!(urls.get(2).is_some_and(|url| url.ends_with("&page=2")));
        Ok(())
    }

    #[test]
    fn adds_details_and_tolerates_individual_failures() -> Result<()> {
        let details = r#"{"pageProps":{"listing":{"propertyOverview":[{"label":"Available From","text":"Now"},{"label":"Bathroom","text":"2"}]}}}"#;
        let (base, server) = crate::test_support::run_server(vec![
            (200, details.to_string()),
            (500, String::new()),
        ])?;
        let mut listings: Vec<Listing> = ["1", "2"]
            .into_iter()
            .map(|id| Listing {
                link: format!("https://www.rent.ie/houses-to-let/slug-{id}/{id}/"),
                ..crate::test_support::listing(id, ListingSource::Rent)
            })
            .collect();

        add_listing_details(&base, &mut listings, "build");

        let first = listings.first().context("expected listing 1")?;
        assert_eq!(first.details.available_from, "Now");
        assert_eq!(first.details.bathrooms, Some(2));
        let second = listings.get(1).context("expected listing 2")?;
        assert_eq!(second.details.available_from, "");

        let requests = server.join().map_err(|_| anyhow!("server panicked"))?;
        assert_eq!(
            requests.first().map(|request| request.url.as_str()),
            Some("/_next/data/build/property.json?id=1&propertyType=houses-to-let&address=slug-1")
        );
        Ok(())
    }
}
