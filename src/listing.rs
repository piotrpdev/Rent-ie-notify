#[derive(Debug, Clone)]
pub(crate) struct Listing {
    pub(crate) id: String,
    pub(crate) link: String,
    pub(crate) img_url: String,
    pub(crate) address: String,
    pub(crate) price: String,
    pub(crate) date: String,
    pub(crate) description: String,
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

pub(crate) fn format_publish_date(timestamp_ms: u64) -> String {
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

#[cfg(test)]
mod tests {
    use super::format_publish_date;

    #[test]
    fn formats_publish_date_in_utc() {
        assert_eq!(format_publish_date(1_794_588_353_317), "2026-11-13");
        assert_eq!(format_publish_date(0), "1970-01-01");
    }
}
