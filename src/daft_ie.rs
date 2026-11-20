use anyhow::{Context as _, Result};
use nanoserde::DeJson;
use reqwest::blocking::Client;

use crate::http::{self, Page};
use crate::listing::{Listing, ListingSource, RawListing, SearchResult, Section};

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

fn listing_from_raw(listing: RawListing) -> Listing {
    let link = absolute_url(&listing.seo_friendly_path);
    let section = if listing
        .sections
        .iter()
        .any(|section| section.eq_ignore_ascii_case("Sharing"))
    {
        Section::RoomsToRent
    } else {
        Section::HousesToLet
    };

    listing.into_listing(ListingSource::Daft, link, section)
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

pub(crate) fn extract_page_from_json(json: &str) -> Result<Page> {
    let data: SearchData =
        SearchData::deserialize_json(json).context("Failed to parse daft.ie API JSON")?;

    let next_from = data.paging.and_then(|paging| {
        (paging.next_from > 0 && paging.displaying_to < paging.total_results)
            .then_some(paging.next_from)
    });

    let listings = data
        .listings
        .into_iter()
        .map(|SearchResult { listing }| listing_from_raw(listing))
        .collect();

    Ok((listings, next_from))
}

/// Returns the index of the closing quote of the JSON string starting at `start`.
fn string_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut index = start + 1;
    loop {
        match bytes.get(index)? {
            b'\\' => index += 2,
            b'"' => return Some(index),
            _ => index += 1,
        }
    }
}

/// Returns the end (exclusive) of the JSON value starting at `start`.
fn value_end(json: &str, start: usize) -> Option<usize> {
    let bytes = json.as_bytes();
    match bytes.get(start)? {
        b'"' => string_end(bytes, start).map(|end| end + 1),
        b'{' | b'[' => {
            let mut depth = 0usize;
            let mut index = start;
            loop {
                match bytes.get(index)? {
                    b'"' => index = string_end(bytes, index)?,
                    b'{' | b'[' => depth += 1,
                    b'}' | b']' => {
                        depth = depth.checked_sub(1)?;
                        if depth == 0 {
                            return Some(index + 1);
                        }
                    }
                    _ => {}
                }
                index += 1;
            }
        }
        _ => Some(
            json.get(start..)?
                .find(|character: char| {
                    matches!(character, ',' | '}' | ']') || character.is_whitespace()
                })
                .map_or(json.len(), |offset| start + offset),
        ),
    }
}

/// Returns the start of the value for `key` among the keys of the outermost
/// object in `json`, ignoring nested objects and string contents.
fn find_key(json: &str, key: &str) -> Option<usize> {
    let bytes = json.as_bytes();
    let mut depth = 0usize;
    let mut index = 0;

    while let Some(&byte) = bytes.get(index) {
        match byte {
            b'"' => {
                let end = string_end(bytes, index)?;
                let rest = json.get(end + 1..)?.trim_start();
                if depth == 1 && json.get(index + 1..end) == Some(key) && rest.starts_with(':') {
                    let value = rest.get(1..)?.trim_start();
                    return Some(json.len() - value.len());
                }
                index = end;
            }
            b'{' | b'[' => depth += 1,
            b'}' | b']' => depth = depth.checked_sub(1)?,
            _ => {}
        }
        index += 1;
    }

    None
}

/// Sets `paging.from` in the top-level `paging` object of a daft.ie request
/// body, inserting it if absent. Other `from` fields (e.g. in `ranges`) are
/// never touched.
pub(crate) fn request_body_with_offset(request_body: &str, offset: u32) -> Result<String> {
    let paging_start = find_key(request_body, "paging")
        .filter(|&start| request_body.as_bytes().get(start) == Some(&b'{'))
        .context("daft.ie request body has no top-level paging object")?;
    let paging_end =
        value_end(request_body, paging_start).context("unterminated daft.ie paging object")?;
    let paging = request_body
        .get(paging_start..paging_end)
        .context("invalid daft.ie paging object")?;

    let (value_start, value_end) = match find_key(paging, "from") {
        Some(from) => {
            let end = value_end(paging, from).context("unterminated daft.ie paging offset")?;
            (paging_start + from, paging_start + end)
        }
        None => (paging_start + 1, paging_start + 1),
    };
    let replacement = if value_start == value_end {
        let has_fields = paging
            .get(1..paging.len() - 1)
            .is_some_and(|inner| !inner.trim().is_empty());
        format!("\"from\":{offset}{}", if has_fields { "," } else { "" })
    } else {
        offset.to_string()
    };

    let before = request_body
        .get(..value_start)
        .context("invalid daft.ie paging offset")?;
    let after = request_body
        .get(value_end..)
        .context("invalid daft.ie paging offset")?;
    Ok(format!("{before}{replacement}{after}"))
}

/// Fetches every results page for a daft.ie gateway request body.
pub(crate) fn fetch_listings(api_url: &str, request_body: &str) -> Result<Vec<Listing>> {
    let client = Client::new();

    http::paginate("daft.ie", |offset| {
        let body = match offset {
            None => request_body.to_string(),
            Some(offset) => request_body_with_offset(request_body, offset)
                .context("Failed to update daft.ie paging")?,
        };
        // The daft gateway is strict about browser-like headers: without
        // brand/platform it returns 400, and without origin/referer it returns 403.
        let request = client
            .post(api_url)
            .header("accept", "application/json")
            .header("brand", "daft")
            .header("content-type", "application/json")
            .header("origin", DAFT_IE_BASE_URL)
            .header("platform", "web")
            .header("referer", "https://www.daft.ie/")
            .body(body);
        let json = http::send_text(request, "daft.ie listings")?;
        extract_page_from_json(&json)
            .context("Failed to extract daft.ie listings from API response")
    })
}

#[cfg(test)]
mod tests {
    use anyhow::{Context as _, Result, anyhow, bail};

    use super::{extract_page_from_json, fetch_listings, request_body_with_offset};
    const SEARCH_JSON: &str = r#"{"listings":[{"listing":{"id":6710820,"title":"Rockshire Road, Ferrybank, Co. Waterford","price":"€1,800 per month","numBedrooms":"2 Bed","numBathrooms":"1 Bath","propertyType":"Apartment","media":{"images":[{"size720x480":"https://media.daft.ie/example.jpg","size72x52":"https://media.daft.ie/thumb.jpg"}]},"seoFriendlyPath":"/for-rent/rockshire-road-ferrybank-co-waterford/6710820","publishDate":1794576513203}}]}"#;

    #[test]
    fn extracts_listing_from_api_json() -> Result<()> {
        let (listings, _) = extract_page_from_json(SEARCH_JSON)?;
        assert_eq!(listings.len(), 1);

        let Some(listing) = listings.first() else {
            bail!("expected one listing");
        };

        assert_eq!(listing.id, "6710820");
        assert_eq!(
            listing.link,
            "https://www.daft.ie/for-rent/rockshire-road-ferrybank-co-waterford/6710820"
        );
        assert_eq!(listing.img_url, "https://media.daft.ie/example.jpg");
        assert_eq!(listing.address, "Rockshire Road, Ferrybank, Co. Waterford");
        assert_eq!(listing.price, "€1,800 per month");
        assert_eq!(listing.date, "13th of November, 2026");
        assert_eq!(listing.description, "2 Bed · 1 Bath · Apartment");

        Ok(())
    }

    #[test]
    fn updates_the_request_body_paging_offset() -> Result<()> {
        let body = r#"{"section":"residential-to-rent","paging":{"from":"0","pageSize":"200"},"terms":""}"#;
        assert_eq!(
            request_body_with_offset(body, 20)?,
            r#"{"section":"residential-to-rent","paging":{"from":20,"pageSize":"200"},"terms":""}"#
        );

        Ok(())
    }

    #[test]
    fn only_rewrites_the_top_level_paging_from() -> Result<()> {
        let body = r#"{"terms":"\"paging\": {\"from\": 1}","paging":{"pageSize":"50"},"ranges":[{"from":"0","to":"1800","name":"rentalPrice"}]}"#;
        assert_eq!(
            request_body_with_offset(body, 50)?,
            r#"{"terms":"\"paging\": {\"from\": 1}","paging":{"from":50,"pageSize":"50"},"ranges":[{"from":"0","to":"1800","name":"rentalPrice"}]}"#
        );

        let body = r#"{"ranges":[{"from":"0"}], "paging" : { "pageSize": 50, "from" : 0 } }"#;
        assert_eq!(
            request_body_with_offset(body, 100)?,
            r#"{"ranges":[{"from":"0"}], "paging" : { "pageSize": 50, "from" : 100 } }"#
        );

        assert_eq!(
            request_body_with_offset(r#"{"paging":{}}"#, 50)?,
            r#"{"paging":{"from":50}}"#
        );
        assert!(request_body_with_offset(r#"{"ranges":[{"paging":{"from":0}}]}"#, 50).is_err());

        Ok(())
    }

    #[test]
    fn extracts_the_next_offset_from_paging() -> Result<()> {
        let json =
            r#"{"listings":[],"paging":{"nextFrom":20,"displayingTo":20,"totalResults":24}}"#;
        assert_eq!(extract_page_from_json(json)?.1, Some(20));

        let json = r#"{"listings":[],"paging":{"nextFrom":0,"displayingTo":24,"totalResults":24}}"#;
        assert_eq!(extract_page_from_json(json)?.1, None);

        Ok(())
    }

    #[test]
    #[ignore = "uses the local archives/ fixture"]
    fn parses_archived_search_response() -> Result<()> {
        let json = std::fs::read_to_string("archives/daftie/listings.json")
            .context("failed to read archived daft.ie API response")?;
        let (listings, _) = extract_page_from_json(&json)?;

        assert_eq!(listings.len(), 20);
        let Some(first) = listings.first() else {
            bail!("expected at least one listing");
        };
        assert_eq!(first.id, "6710820");
        assert_eq!(
            first.link,
            "https://www.daft.ie/for-rent/rockshire-road-ferrybank-co-waterford/6710820"
        );
        assert_eq!(first.date, "13th of November, 2026");
        assert_eq!(first.description, "2 Bed · 1 Bath · Apartment");

        let (_, next_from) = extract_page_from_json(&json)?;
        assert_eq!(next_from, Some(20));

        let large_page_json =
            std::fs::read_to_string("archives/daftie/listings_page_size_200.json")
                .context("failed to read daft.ie 200-page fixture")?;
        let (listings, next_from) = extract_page_from_json(&large_page_json)?;
        assert_eq!(listings.len(), 24);
        assert_eq!(next_from, None);

        Ok(())
    }

    #[test]
    #[ignore = "uses the local archives/ fixtures"]
    fn parses_archived_paginated_search() -> Result<()> {
        let read = |path: &str| {
            std::fs::read_to_string(path).with_context(|| format!("failed to read {path}"))
        };
        let (page_one, next_from) = extract_page_from_json(&read(
            "archives/daftie/residential_waterford_city_rent_0-1800_page_1.json",
        )?)?;
        assert_eq!(page_one.len(), 20);
        assert_eq!(next_from, Some(20));
        assert_eq!(
            page_one.first().and_then(|listing| listing.bedrooms),
            Some(2)
        );
        // Not every daft.ie listing has a map point.
        let with_coordinates = page_one
            .iter()
            .filter(|listing| listing.coordinates.is_some())
            .count();
        assert_eq!(with_coordinates, 17);

        let (page_two, next_from) = extract_page_from_json(&read(
            "archives/daftie/residential_waterford_city_rent_0-1800_page_2.json",
        )?)?;
        assert_eq!(page_two.len(), 11);
        assert_eq!(next_from, None);

        let ids: std::collections::HashSet<&str> = page_one
            .iter()
            .chain(&page_two)
            .map(|listing| listing.id.as_str())
            .collect();
        assert_eq!(ids.len(), 31);

        Ok(())
    }

    #[test]
    fn follows_next_from_with_the_required_headers() -> Result<()> {
        let page = |id: u32, next_from: u32| {
            format!(
                r#"{{"listings":[{{"listing":{{"id":{id},"title":"Listing {id}","price":"€1","seoFriendlyPath":"/for-rent/slug-{id}/{id}"}}}}],"paging":{{"nextFrom":{next_from},"displayingTo":{id},"totalResults":2}}}}"#
            )
        };
        let (base, server) =
            crate::test_support::run_server(vec![(200, page(1, 1)), (200, page(2, 0))])?;

        let listings = fetch_listings(
            &format!("{base}/api/v2/ads/listings"),
            r#"{"section":"residential-to-rent","paging":{"from":"0","pageSize":"1"}}"#,
        )?;

        let ids: Vec<&str> = listings.iter().map(|listing| listing.id.as_str()).collect();
        assert_eq!(ids, vec!["1", "2"]);

        let requests = server.join().map_err(|_| anyhow!("server panicked"))?;
        let second = requests.get(1).context("expected a second request")?;
        assert!(
            second
                .body
                .contains(r#""paging":{"from":1,"pageSize":"1"}"#)
        );
        for request in &requests {
            assert_eq!(request.header("brand"), Some("daft"));
            assert_eq!(request.header("platform"), Some("web"));
            assert_eq!(request.header("origin"), Some("https://www.daft.ie"));
        }
        Ok(())
    }
}
