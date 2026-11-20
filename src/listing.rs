use base64::{Engine as _, engine::general_purpose};

use nanoserde::DeJson;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ListingSource {
    Rent,
    Daft,
    Property,
}

impl ListingSource {
    pub(crate) const ALL: [Self; 3] = [Self::Rent, Self::Daft, Self::Property];
}

/// The rent.ie section (`propertyType`) a listing belongs to. Property-detail
/// requests for every site go through rent.ie and need it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Section {
    HousesToLet,
    RoomsToRent,
    /// Any other rent.ie section name, passed through as-is.
    Other(String),
}

impl Section {
    /// Parses a rent.ie section slug; an empty slug means houses to let.
    pub(crate) fn from_slug(slug: &str) -> Self {
        match slug {
            "" | "houses-to-let" => Self::HousesToLet,
            "rooms-to-rent" => Self::RoomsToRent,
            other => Self::Other(other.to_string()),
        }
    }

    /// The rent.ie URL slug, e.g. `houses-to-let`.
    pub(crate) fn slug(&self) -> &str {
        match self {
            Self::HousesToLet => "houses-to-let",
            Self::RoomsToRent => "rooms-to-rent",
            Self::Other(slug) => slug,
        }
    }

    /// Whether this is a room in a shared property rather than a whole home.
    pub(crate) const fn is_room_share(&self) -> bool {
        matches!(self, Self::RoomsToRent)
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Coordinates {
    pub(crate) latitude: f64,
    pub(crate) longitude: f64,
}

#[derive(Debug, Clone)]
pub(crate) struct Listing {
    pub(crate) id: String,
    pub(crate) source: ListingSource,
    pub(crate) link: String,
    pub(crate) img_url: String,
    pub(crate) image_keys: Vec<String>,
    pub(crate) coordinates: Option<Coordinates>,
    pub(crate) address: String,
    pub(crate) price: String,
    pub(crate) date: String,
    pub(crate) description: String,
    /// Numeric bedroom count from the search API (`None` for room shares).
    pub(crate) bedrooms: Option<u32>,
    pub(crate) details: ListingDetails,
    pub(crate) section: Section,
}

/// Fields from the rent.ie property-detail endpoint; empty until fetched.
#[derive(Debug, Clone, Default)]
pub(crate) struct ListingDetails {
    pub(crate) available_from: String,
    pub(crate) minimum_lease: String,
    pub(crate) single_bedrooms: Option<u32>,
    pub(crate) double_bedrooms: Option<u32>,
    pub(crate) twin_bedrooms: Option<u32>,
    pub(crate) bathrooms: Option<u32>,
}

/// A search result entry as returned by all three sites' search APIs (they
/// share a backend, so the listing schema is the same). Fields that only one
/// site sends (`sections`, `point`: daft.ie) are optional.
///
/// nanoserde-derive panics on `pub(crate)`, so these types are `pub`.
#[derive(Debug, DeJson)]
pub struct SearchResult {
    pub listing: RawListing,
}

#[derive(Debug, DeJson)]
pub struct RawListing {
    id: u64,
    title: String,
    price: String,
    #[nserde(rename = "numBedrooms")]
    num_bedrooms: Option<String>,
    #[nserde(rename = "numBathrooms")]
    num_bathrooms: Option<String>,
    #[nserde(rename = "propertyType")]
    property_type: Option<String>,
    media: Option<RawMedia>,
    #[nserde(rename = "seoFriendlyPath")]
    pub seo_friendly_path: String,
    #[nserde(rename = "publishDate")]
    publish_date: Option<u64>,
    /// daft.ie only, e.g. `["Sharing"]`.
    #[nserde(default)]
    pub sections: Vec<String>,
    /// daft.ie only: coordinates in `[longitude, latitude]` order.
    point: Option<GeoPoint>,
}

#[derive(Debug, DeJson)]
struct GeoPoint {
    coordinates: Vec<f64>,
}

#[derive(Debug, DeJson)]
struct RawMedia {
    #[nserde(default)]
    images: Vec<RawImage>,
}

#[derive(Debug, DeJson)]
struct RawImage {
    #[nserde(rename = "size720x480")]
    large: Option<String>,
    #[nserde(rename = "size360x240")]
    medium: Option<String>,
    #[nserde(rename = "size72x52")]
    small: Option<String>,
}

impl RawImage {
    /// The image URL each site has always used: daft.ie's large (or small)
    /// rendition, and the 360x240 thumbnail elsewhere. Image keys for
    /// cross-site matching are decoded from these URLs.
    fn url(self, source: ListingSource) -> Option<String> {
        match source {
            ListingSource::Daft => self.large.or(self.small),
            ListingSource::Rent | ListingSource::Property => self.medium,
        }
    }
}

impl RawListing {
    pub(crate) const fn id(&self) -> u64 {
        self.id
    }

    /// Builds a [`Listing`]. `link` and `section` are site specific and
    /// computed by the caller.
    pub(crate) fn into_listing(
        self,
        source: ListingSource,
        link: String,
        section: Section,
    ) -> Listing {
        let image_urls: Vec<String> = self
            .media
            .into_iter()
            .flat_map(|media| media.images)
            .filter_map(|image| image.url(source))
            .collect();
        let coordinates = self.point.and_then(|point| {
            Some(Coordinates {
                longitude: point.coordinates.first().copied()?,
                latitude: point.coordinates.get(1).copied()?,
            })
        });

        Listing {
            // rent.ie and daft.ie share the numeric listing ID space.
            id: self.id.to_string(),
            source,
            link,
            img_url: image_urls.first().cloned().unwrap_or_default(),
            image_keys: extract_image_keys(&image_urls),
            coordinates,
            address: self.title.trim().to_string(),
            price: self.price.trim().to_string(),
            date: self.publish_date.map_or_default(format_publish_date),
            bedrooms: bedroom_count(self.num_bedrooms.as_deref()),
            description: format_description([
                self.num_bedrooms,
                self.num_bathrooms,
                self.property_type,
            ]),
            details: ListingDetails::default(),
            section,
        }
    }
}

#[derive(Debug, DeJson)]
struct ImageDescriptor {
    key: Option<String>,
}

fn image_key(url: &str) -> Option<String> {
    let without_query = url.split('?').next()?;
    let (_, address) = without_query.split_once("://")?;
    let (_, payload) = address.split_once('/')?;
    let decoded = general_purpose::STANDARD
        .decode(payload)
        .or_else(|_| general_purpose::URL_SAFE.decode(payload))
        .or_else(|_| general_purpose::URL_SAFE_NO_PAD.decode(payload))
        .ok()?;
    let json = String::from_utf8(decoded).ok()?;
    ImageDescriptor::deserialize_json(&json).ok()?.key
}

fn extract_image_keys(urls: &[String]) -> Vec<String> {
    let mut keys = Vec::new();

    for url in urls {
        if let Some(key) = image_key(url)
            && !keys.contains(&key)
        {
            keys.push(key);
        }
    }

    keys
}

fn format_description(parts: [Option<String>; 3]) -> String {
    parts
        .into_iter()
        .flatten()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

fn ordinal_suffix(day: i64) -> &'static str {
    if (11..=13).contains(&(day % 100)) {
        return "th";
    }

    match day % 10 {
        1 => "st",
        2 => "nd",
        3 => "rd",
        _ => "th",
    }
}

const fn month_name(month: i64) -> &'static str {
    match month {
        1 => "January",
        2 => "February",
        3 => "March",
        4 => "April",
        5 => "May",
        6 => "June",
        7 => "July",
        8 => "August",
        9 => "September",
        10 => "October",
        11 => "November",
        12 => "December",
        _ => "",
    }
}

/// Parses the leading number of a `numBedrooms` value such as `"3 Bed"`.
/// Room-share values like `"Double Room"` have no count.
pub(crate) fn bedroom_count(num_bedrooms: Option<&str>) -> Option<u32> {
    num_bedrooms?.split_whitespace().next()?.parse().ok()
}

fn format_publish_date(timestamp_ms: u64) -> String {
    let days_since_epoch = i64::try_from(timestamp_ms / 86_400_000).unwrap_or_default();
    let (year, month, day) = civil_from_days(days_since_epoch);

    format!(
        "{day}{} of {}, {year}",
        ordinal_suffix(day),
        month_name(month)
    )
}

/// Converts days since 1970-01-01 into a `(year, month, day)` UTC date.
pub(crate) const fn civil_from_days(days_since_epoch: i64) -> (i64, i64, i64) {
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

#[cfg(test)]
mod tests {
    use super::{Section, bedroom_count, extract_image_keys, format_publish_date};

    #[test]
    fn formats_publish_date_in_utc() {
        assert_eq!(
            format_publish_date(1_794_588_353_317),
            "13th of November, 2026"
        );
        assert_eq!(format_publish_date(0), "1st of January, 1970");
    }

    #[test]
    fn formats_ordinal_date_suffixes() {
        assert_eq!(format_publish_date(86_400_000), "2nd of January, 1970");
        assert_eq!(format_publish_date(172_800_000), "3rd of January, 1970");
        assert_eq!(format_publish_date(864_000_000), "11th of January, 1970");
        assert_eq!(format_publish_date(1_728_000_000), "21st of January, 1970");
        assert_eq!(format_publish_date(1_814_400_000), "22nd of January, 1970");
        assert_eq!(format_publish_date(1_900_800_000), "23rd of January, 1970");
    }

    #[test]
    fn parses_numeric_bedroom_counts_from_descriptions() {
        assert_eq!(bedroom_count(Some("16 Bed")), Some(16));
        assert_eq!(bedroom_count(Some("Double Room")), None);
        assert_eq!(bedroom_count(None), None);
    }

    #[test]
    fn extracts_image_keys_from_media_urls() {
        let url = "https://media.example/eyJrZXkiOiJhL2IvYy5qcGcifQ==?signature=x";
        let keys = extract_image_keys(&[url.to_string(), url.to_string()]);
        assert_eq!(keys, vec!["a/b/c.jpg"]);

        let keys = extract_image_keys(&["https://media.example/example.jpg".to_string()]);
        assert_eq!(keys, Vec::<String>::new());
    }

    #[test]
    fn parses_rent_ie_sections() {
        assert_eq!(Section::from_slug("rooms-to-rent"), Section::RoomsToRent);
        assert_eq!(Section::from_slug(""), Section::HousesToLet);
        let other = Section::from_slug("apartments-to-let");
        assert_eq!(other.slug(), "apartments-to-let");
        assert!(!other.is_room_share());
        assert!(Section::RoomsToRent.is_room_share());
    }
}
