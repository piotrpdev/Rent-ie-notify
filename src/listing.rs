use base64::{Engine as _, engine::general_purpose};

use nanoserde::DeJson;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ListingSource {
    RentIe,
    DaftIe,
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

pub(crate) fn extract_image_keys(urls: &[String]) -> Vec<String> {
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

pub(crate) fn format_description(parts: [Option<String>; 3]) -> String {
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

pub(crate) fn format_publish_date(timestamp_ms: u64) -> String {
    let days_since_epoch = i64::try_from(timestamp_ms / 86_400_000).unwrap_or_default();
    let (year, month, day) = civil_from_days(days_since_epoch);

    format!(
        "{day}{} of {}, {year}",
        ordinal_suffix(day),
        month_name(month)
    )
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

#[cfg(test)]
mod tests {
    use super::{extract_image_keys, format_publish_date};

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
    fn extracts_image_keys_from_media_urls() {
        let url = "https://media.example/eyJrZXkiOiJhL2IvYy5qcGcifQ==?signature=x";
        let keys = extract_image_keys(&[url.to_string(), url.to_string()]);
        assert_eq!(keys, vec!["a/b/c.jpg"]);

        let keys = extract_image_keys(&["https://media.example/example.jpg".to_string()]);
        assert_eq!(keys, Vec::<String>::new());
    }
}
