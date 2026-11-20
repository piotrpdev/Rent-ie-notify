use anyhow::{Context as _, Result};
use nanoserde::DeJson;
use reqwest::blocking::Client;

use crate::http::{self, Page};
use crate::listing::{Listing, ListingSource, SearchResult};
use crate::next_data::{self, Paging};

/// Public site, used for listing links.
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

/// The rent.ie detail-endpoint section for a property.ie ad type.
fn detail_property_type(ad_type: &str) -> &'static str {
    if ad_type.contains("share") {
        "rooms-to-rent"
    } else {
        "houses-to-let"
    }
}

fn listing_url(ad_type: &str, seo_friendly_path: &str, listing_id: u64) -> String {
    let slug = next_data::slug_from_path(seo_friendly_path).trim_matches('-');

    format!("{PROPERTY_IE_BASE_URL}/{ad_type}/{slug}/{listing_id}/")
}

fn parse_next_page_data(html: &str) -> Result<NextPageData> {
    let json = next_data::json_from_html(html)?;

    NextPageData::deserialize_json(&json).context("Failed to parse __NEXT_DATA__ JSON")
}

pub(crate) fn search_api_url(base_url: &str, html: &str) -> Result<String> {
    let page_data = parse_next_page_data(html)?;
    let route = next_data::data_route(&page_data.page, &page_data.query.rest);

    let mut url = format!(
        "{base_url}/_next/data/{}/{route}.json?adType={}&county={}",
        page_data.build_id, page_data.query.ad_type, page_data.query.county
    );

    for segment in &page_data.query.rest {
        url.push_str("&rest=");
        url.push_str(segment);
    }

    Ok(url)
}

pub(crate) fn extract_page_from_json(json: &str) -> Result<Page> {
    let data: SearchData =
        SearchData::deserialize_json(json).context("Failed to parse property.ie API JSON")?;
    let SearchPageProps {
        listings,
        ad_type,
        paging,
    } = data.page_props;

    let next_page = paging.as_ref().and_then(Paging::next_page);

    let listings = listings
        .into_iter()
        .map(|SearchResult { listing }| {
            let link = listing_url(&ad_type, &listing.seo_friendly_path, listing.id());
            listing.into_listing(
                ListingSource::Property,
                link,
                detail_property_type(&ad_type),
            )
        })
        .collect();

    Ok((listings, next_page))
}

/// Fetches every results page for a property.ie search URL.
pub(crate) fn fetch_listings(base_url: &str, search_url: &str) -> Result<Vec<Listing>> {
    // property.ie requires HTTP/2; Cloudflare returns 403 for HTTP/1.1 requests.
    let client = Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .context("Failed to create property.ie HTTP client")?;

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
    let api_url = search_api_url(base_url, &html)?;

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
    use anyhow::{Context as _, Result, anyhow, bail};

    use super::{extract_page_from_json, fetch_listings, listing_url, search_api_url};
    const PAGE_HTML: &str = r#"<!DOCTYPE html>
<html>
<head>
<script id="__NEXT_DATA__" type="application/json">{"props":{},"page":"/search/[[...rest]]","query":{"adType":"property-to-let","county":"waterford","rest":["price_0-3500","beds_4"]},"buildId":"test-build"}</script>
</head>
<body></body>
</html>"#;

    const SEARCH_JSON: &str = r#"{"pageProps":{"adType":"property-to-let","paging":{"currentPage":1,"totalPages":2},"listings":[{"listing":{"id":6721266,"title":"Lakunda, West Street, Tallow, Co. Waterford","price":"€1,600 per month","numBedrooms":"4 Bed","numBathrooms":"1 Bath","propertyType":"House","media":{"images":[{"size360x240":"https://media.property.ie/example.jpg"}]},"seoFriendlyPath":"/for-rent/lakunda-west-street-tallow-co-waterford/6721266","publishDate":1794564618513}}]}}"#;

    #[test]
    fn builds_the_property_ie_search_api_url() -> Result<()> {
        assert_eq!(
            search_api_url("https://www.property.ie", PAGE_HTML)?,
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
    fn extracts_a_property_ie_listing() -> Result<()> {
        let (listings, next_page) = extract_page_from_json(SEARCH_JSON)?;
        assert_eq!(listings.len(), 1);
        assert_eq!(next_page, Some(2));

        let Some(listing) = listings.first() else {
            bail!("expected a listing");
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
    fn parses_archived_search_and_details() -> Result<()> {
        let read = |path: &str| {
            std::fs::read_to_string(path).with_context(|| format!("failed to read {path}"))
        };
        let (listings, next_page) = extract_page_from_json(&read(
            "archives/propertyie/search_waterford_4beds_0-3500_page_1.json",
        )?)?;
        assert_eq!(listings.len(), 18);
        assert_eq!(next_page, None);
        let Some(first) = listings.first() else {
            bail!("expected listings");
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

    #[test]
    fn fetches_the_search_page_then_the_api() -> Result<()> {
        let page = SEARCH_JSON.replace(r#""totalPages":2"#, r#""totalPages":1"#);
        let (base, server) =
            crate::test_support::run_server(vec![(200, PAGE_HTML.to_string()), (200, page)])?;

        let listings = fetch_listings(&base, &format!("{base}/property-to-let/waterford/"))?;

        assert_eq!(listings.len(), 1);
        let requests = server.join().map_err(|_| anyhow!("server panicked"))?;
        assert!(requests.get(1).is_some_and(|request| {
            request
                .url
                .starts_with("/_next/data/test-build/search/price_0-3500/beds_4.json?")
        }));
        Ok(())
    }
}
