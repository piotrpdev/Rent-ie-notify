use nanoserde::DeJson;
use tl::{Node, ParserOptions};

const NEXT_DATA_SELECTOR: &str = "script#__NEXT_DATA__";
const RENT_IE_BASE_URL: &str = "https://www.rent.ie";

#[derive(Debug, Clone)]
pub(crate) struct RentListing {
    pub(crate) id: String,
    pub(crate) link: String,
    pub(crate) img_url: String,
    pub(crate) address: String,
    pub(crate) price: String,
    pub(crate) date: String,
    pub(crate) description: String,
}

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

impl RentListing {
    fn from_listing(listing: ListingData, section: &str) -> Self {
        let ListingData {
            id,
            title,
            price,
            num_bedrooms,
            property_type,
            media,
            seo_friendly_path,
            publish_date,
        } = listing;

        let img_url = media
            .and_then(|media| media.images.into_iter().next())
            .and_then(|image| image.thumbnail)
            .unwrap_or_default();

        let description = [num_bedrooms, property_type]
            .into_iter()
            .flatten()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>()
            .join(" · ");

        Self {
            id: id.to_string(),
            link: listing_url(section, &seo_friendly_path, id),
            img_url,
            address: title.trim().to_string(),
            price: price.trim().to_string(),
            date: publish_date.map(format_publish_date).unwrap_or_default(),
            description,
        }
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

fn format_publish_date(timestamp_ms: u64) -> String {
    let days_since_epoch = i64::try_from(timestamp_ms / 86_400_000).unwrap_or_default();
    let (year, month, day) = civil_from_days(days_since_epoch);

    format!("{year:04}-{month:02}-{day:02}")
}

/// Converts days since 1970-01-01 into a `(year, month, day)` UTC date.
const fn civil_from_days(days_since_epoch: i64) -> (i64, i64, i64) {
    let shifted = days_since_epoch + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = if month <= 2 { year + 1 } else { year };

    (year, month, day)
}

pub(crate) fn extract_listings_from_json(json: &str) -> Result<Vec<RentListing>, String> {
    let data: SearchData =
        SearchData::deserialize_json(json).map_err(|e| format!("Failed to parse API JSON: {e}"))?;
    let SearchPageProps {
        listings,
        property_type,
    } = data.page_props;

    Ok(listings
        .into_iter()
        .map(|search_result| RentListing::from_listing(search_result.listing, &property_type))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::{extract_listings_from_json, format_publish_date, listing_url, search_api_url};

    const PAGE_HTML: &str = r#"<!DOCTYPE html>
<html>
<head>
<script id="__NEXT_DATA__" type="application/json">{"props":{"pageProps":{}},"page":"/search/[[...rest]]","query":{"propertyType":"rooms-to-rent","county":"waterford","areaFamily":"locality","rest":["waterford-city-centre","room-type_either","sort-by_date-entered_down"]},"buildId":"test-build"}</script>
</head>
<body></body>
</html>"#;

    const SEARCH_JSON: &str = r#"{"pageProps":{"propertyType":"rooms-to-rent","listings":[{"listing":{"id":6721798,"title":" Thomas Street, Waterford, Ferrybank, Co. Waterford","price":"€650 per month","numBedrooms":"Double Room","propertyType":"House","media":{"images":[{"size360x240":"https://media.rent.ie/example.jpg"}]},"seoFriendlyPath":"/share/thomas-street-waterford/6721798","publishDate":1794588353317}}]}}"#;

    #[test]
    fn builds_the_search_api_url_from_page_data() -> Result<(), String> {
        assert_eq!(
            search_api_url(PAGE_HTML)?,
            "https://www.rent.ie/_next/data/test-build/search/waterford-city-centre/room-type_either/sort-by_date-entered_down.json?propertyType=rooms-to-rent&county=waterford&areaFamily=locality&rest=waterford-city-centre&rest=room-type_either&rest=sort-by_date-entered_down"
        );

        Ok(())
    }

    #[test]
    fn extracts_listing_from_api_json() -> Result<(), String> {
        let listings = extract_listings_from_json(SEARCH_JSON)?;
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
        assert_eq!(listing.date, "2026-11-13");
        assert_eq!(listing.description, "Double Room · House");

        Ok(())
    }

    #[test]
    fn formats_publish_date_in_utc() {
        assert_eq!(format_publish_date(1_794_588_353_317), "2026-11-13");
        assert_eq!(format_publish_date(0), "1970-01-01");
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
    #[ignore = "uses the local archives/ fixture"]
    fn parses_archived_search_response() -> Result<(), String> {
        let json = std::fs::read_to_string("archives/search_waterford_city_centre.json")
            .map_err(|e| format!("failed to read archived API response: {e}"))?;
        let listings = extract_listings_from_json(&json)?;

        assert_eq!(listings.len(), 20);
        let Some(first) = listings.first() else {
            return Err("expected at least one listing".to_string());
        };
        assert_eq!(
            first.link,
            "https://www.rent.ie/rooms-to-rent/-thomas-street-waterford-ferrybank-co-waterford/6721798/"
        );

        Ok(())
    }
}
