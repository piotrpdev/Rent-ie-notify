use anyhow::{Context as _, Result};
use std::{
    collections::BTreeMap,
    fmt::Write as _,
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::listing::Listing;

fn plural(count: u32, singular: &str, plural: &str) -> String {
    if count == 1 {
        format!("{count} {singular}")
    } else {
        format!("{count} {plural}")
    }
}

fn bedroom_group(listing: &Listing) -> Option<String> {
    let doubles = listing.details.double_bedrooms.unwrap_or(0);
    let singles = listing.details.single_bedrooms.unwrap_or(0);
    let twins = listing.details.twin_bedrooms.unwrap_or(0);

    if doubles == 0 && singles == 0 && twins == 0 {
        return None;
    }

    let mut parts = Vec::new();
    if doubles > 0 {
        parts.push(plural(doubles, "double bed", "double beds"));
    }
    if singles > 0 {
        parts.push(plural(singles, "single bed", "single beds"));
    }
    if twins > 0 {
        parts.push(plural(twins, "twin bed", "twin beds"));
    }

    Some(parts.join(", "))
}

fn compact_price(price: &str) -> String {
    price
        .trim()
        .trim_start_matches('€')
        .replace(" per month", "/month")
        .replace(" per week", "/week")
        .replace(" monthly", "/month")
        .replace(" weekly", "/week")
        .replace('€', "")
}

fn available_text(available_from: &str) -> String {
    let value = available_from.trim();
    if value.is_empty() {
        "unknown".to_string()
    } else if value.eq_ignore_ascii_case("now")
        || value.eq_ignore_ascii_case("available now")
        || value.eq_ignore_ascii_case("immediately")
    {
        "now".to_string()
    } else {
        value.to_string()
    }
}

fn lease_text(minimum_lease: &str) -> String {
    let value = minimum_lease.trim().trim_end_matches('+').trim();
    if value.is_empty() {
        return "min. unknown".to_string();
    }

    let lower = value.to_ascii_lowercase();
    let lower = lower
        .strip_suffix(" lease")
        .unwrap_or(lower.as_str())
        .trim();
    if lower.eq_ignore_ascii_case("no minimum") {
        return "no min. lease".to_string();
    }

    let lower = lower
        .strip_prefix("minimum ")
        .or_else(|| lower.strip_prefix("min. "))
        .or_else(|| lower.strip_prefix("min "))
        .unwrap_or(lower)
        .trim();

    if let Some(years) = lower
        .strip_suffix(" years")
        .or_else(|| lower.strip_suffix(" year"))
    {
        return format!("min. {years}yr lease");
    }
    if let Some(months) = lower
        .strip_suffix(" months")
        .or_else(|| lower.strip_suffix(" month"))
    {
        return format!("min. {months}month lease");
    }

    format!("min. {lower} lease")
}

fn listing_line(listing: &Listing) -> String {
    let bathrooms = listing.details.bathrooms.map_or_else(
        || "? bath".to_string(),
        |count| plural(count, "bath", "bath"),
    );
    let price = compact_price(&listing.price);
    let available = available_text(&listing.details.available_from);
    let lease = lease_text(&listing.details.minimum_lease);

    format!(
        "({bathrooms}, {price}, available {available}, {lease}) {}",
        listing.link
    )
}

pub(crate) fn generate(listings: &[Listing]) -> String {
    let mut groups: BTreeMap<String, Vec<&Listing>> = BTreeMap::new();

    for listing in listings {
        let (heading, is_bed_group) = bedroom_group(listing).map_or_else(
            || {
                let description = listing.description.trim();
                let heading = if description.is_empty() {
                    "Other listings".to_string()
                } else {
                    description.to_string()
                };
                (heading, false)
            },
            |group| (group, true),
        );
        let heading = if is_bed_group {
            format!("Houses with {heading}")
        } else {
            heading
        };
        groups.entry(heading).or_default().push(listing);
    }

    let mut output = String::new();
    for (heading, group) in groups {
        let _ = writeln!(output, "{heading}:");
        for listing in group {
            let _ = writeln!(output, "{}", listing_line(listing));
        }
        output.push('\n');
    }

    output
}

fn filename_from_seconds(seconds: u64) -> String {
    let days = i64::try_from(seconds / 86_400).unwrap_or_default();
    let time_of_day = seconds % 86_400;
    let (year, month, day) = crate::listing::civil_from_days(days);
    let hour = time_of_day / 3_600;
    let minute = (time_of_day % 3_600) / 60;
    let second = time_of_day % 60;

    format!("report-{year:04}-{month:02}-{day:02}_{hour:02}-{minute:02}-{second:02}.txt")
}

fn report_filename() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or_default(|duration| duration.as_secs());

    filename_from_seconds(seconds)
}

pub(crate) fn write_report(dir: &str, listings: &[Listing]) -> Result<String> {
    let directory = Path::new(dir);
    fs::create_dir_all(directory)
        .with_context(|| format!("Failed to create report directory {dir}"))?;

    let path = directory.join(report_filename());
    fs::write(&path, generate(listings))
        .with_context(|| format!("Failed to write report to {}", path.display()))?;

    Ok(path.display().to_string())
}

#[cfg(test)]
mod tests {
    use anyhow::{Context as _, Result};
    use std::path::Path;

    use tempfile::tempdir;

    use super::{filename_from_seconds, generate, write_report};
    use crate::listing::{Listing, ListingDetails, ListingSource};
    use crate::test_support;

    #[allow(clippy::too_many_arguments)]
    fn listing(
        id: &str,
        double_bedrooms: Option<u32>,
        single_bedrooms: Option<u32>,
        twin_bedrooms: Option<u32>,
        bathrooms: Option<u32>,
        price: &str,
        available_from: &str,
        minimum_lease: &str,
    ) -> Listing {
        Listing {
            price: price.to_string(),
            details: ListingDetails {
                available_from: available_from.to_string(),
                minimum_lease: minimum_lease.to_string(),
                single_bedrooms,
                double_bedrooms,
                twin_bedrooms,
                bathrooms,
            },
            ..test_support::listing(id, ListingSource::Rent)
        }
    }

    #[test]
    fn formats_timestamped_report_filenames() {
        assert_eq!(filename_from_seconds(0), "report-1970-01-01_00-00-00.txt");
    }

    #[test]
    fn writes_timestamped_report_into_directory() -> Result<()> {
        let directory = tempdir().context("failed to create temp dir")?;
        let directory_path = directory
            .path()
            .to_str()
            .context("temp directory is not valid UTF-8")?;

        let path = write_report(directory_path, &[])?;

        assert!(Path::new(&path).is_file());
        assert!(path.contains("report-"));

        Ok(())
    }

    #[test]
    fn groups_and_formats_a_report() {
        let listings = vec![
            listing(
                "1",
                Some(4),
                None,
                None,
                Some(1),
                "€2,600 per month",
                "Now",
                "1 Year",
            ),
            listing(
                "2",
                Some(2),
                Some(2),
                None,
                Some(3),
                "€2,100 per month",
                "Nov 21st, 2026",
                "No Minimum",
            ),
            listing(
                "3",
                Some(2),
                Some(1),
                Some(1),
                Some(2),
                "€125 per week",
                "Immediately",
                "Minimum 3 Months",
            ),
        ];

        let report = generate(&listings);

        assert!(report.contains("Houses with 4 double beds:"));
        assert!(report.contains(
            "(1 bath, 2,600/month, available now, min. 1yr lease) https://example.com/1"
        ));
        assert!(report.contains("Houses with 2 double beds, 2 single beds:"));
        assert!(report.contains(
            "(3 bath, 2,100/month, available Nov 21st, 2026, no min. lease) https://example.com/2"
        ));
        assert!(report.contains("Houses with 2 double beds, 1 single bed, 1 twin bed:"));
        assert!(report.contains(
            "(2 bath, 125/week, available now, min. 3month lease) https://example.com/3"
        ));
    }
}
