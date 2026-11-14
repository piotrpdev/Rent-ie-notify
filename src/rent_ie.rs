use nanoserde::DeJson;
use tl::{Node, ParserOptions};

use crate::listing::{
    Listing, ListingSource, extract_image_keys, format_description, format_publish_date,
};

const NEXT_DATA_SELECTOR: &str = "script#__NEXT_DATA__";
const RENT_IE_BASE_URL: &str = "https://www.rent.ie";

#[derive(Debug, DeJson)]
struct NextPageData {
    #[nserde(rename = "buildId")]
    build_id: String,
    page: String,
    query: NextQuery,
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
        source: ListingSource::RentIe,
        link: listing_url(section, &seo_friendly_path, id),
        img_url,
        image_keys,
        coordinates: None,
        address: title.trim().to_string(),
        price: price.trim().to_string(),
        date: publish_date.map(format_publish_date).unwrap_or_default(),
        description: format_description([num_bedrooms, num_bathrooms, property_type]),
    }
}

fn listing_url(section: &str, seo_friendly_path: &str, listing_id: u64) -> String {
    // `seoFriendlyPath` is a "/share/<slug>/<id>" path that doesn't resolve on its own;
    // the public listing page is served from "/<section>/<slug>/<id>/".
    let slug = seo_friendly_path
        .trim_end_matches('/')
        .rsplit('/')
        .nth(1)
        .unwrap_or_default();

    if slug.is_empty() {
        format!("{RENT_IE_BASE_URL}/{section}/{listing_id}/")
    } else {
        format!("{RENT_IE_BASE_URL}/{section}/{slug}/{listing_id}/")
    }
}

fn parse_next_page_data(html: &str) -> Result<NextPageData, String> {
    let dom = tl::parse(html, ParserOptions::default())
        .map_err(|e| format!("Failed to parse HTML: {e}"))?;

    let next_data_script = dom
        .query_selector(NEXT_DATA_SELECTOR)
        .and_then(|mut selector| selector.next())
        .and_then(|node_handle| node_handle.get(dom.parser()))
        .and_then(Node::as_tag)
        .ok_or("Failed to find __NEXT_DATA__ script in HTML")?;

    let json = next_data_script.inner_text(dom.parser());

    NextPageData::deserialize_json(&json)
        .map_err(|e| format!("Failed to parse __NEXT_DATA__ JSON: {e}"))
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

pub(crate) fn extract_page_from_json(json: &str) -> Result<(Vec<Listing>, Option<u32>), String> {
    let data: SearchData =
        SearchData::deserialize_json(json).map_err(|e| format!("Failed to parse API JSON: {e}"))?;
    let SearchPageProps {
        listings,
        property_type,
        paging,
    } = data.page_props;

    let next_page = paging.and_then(|paging| {
        (paging.current_page < paging.total_pages).then_some(paging.current_page + 1)
    });

    let listings = listings
        .into_iter()
        .map(|search_result| listing_from_data(search_result.listing, &property_type))
        .collect();

    Ok((listings, next_page))
}

#[cfg(test)]
mod tests {
    use super::{extract_page_from_json, listing_url, search_api_url};

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
}
