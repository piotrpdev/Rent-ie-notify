//! `sent_ids.txt`: the IDs of every listing already posted to Slack.

use std::{collections::HashSet, fs::OpenOptions, io::Write, path::Path};

pub(crate) const SENT_IDS_FILENAME: &str = "sent_ids.txt";

fn parse(content: &str) -> HashSet<String> {
    // Older versions stored "<slug>/<id>" and later versions stored daft IDs as
    // "daft-<id>". Reduce both formats to the shared numeric ID.
    content
        .lines()
        .map(|line| line.trim().rsplit('/').next().unwrap_or_default())
        .map(|id| id.strip_prefix("daft-").unwrap_or(id))
        .filter(|id| !id.is_empty())
        .map(ToString::to_string)
        .collect()
}

/// Loads previously sent IDs. A missing file means nothing has been sent yet.
pub(crate) fn load(path: &Path) -> HashSet<String> {
    std::fs::read_to_string(path).map_or_else(
        |_| {
            eprintln!(
                "{} not found, assuming no listings have been sent yet.",
                path.display()
            );
            HashSet::new()
        },
        |content| parse(&content),
    )
}

/// Appends `ids`, one per line, creating the file if needed.
pub(crate) fn append(path: &Path, ids: &[String]) -> Result<(), String> {
    if ids.is_empty() {
        return Ok(());
    }

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| format!("Failed to open {}: {error}", path.display()))?;

    let mut content = ids.join("\n");
    content.push('\n');
    file.write_all(content.as_bytes())
        .map_err(|error| format!("Failed to write {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::{append, load, parse};

    #[test]
    fn parses_legacy_and_current_listing_ids() {
        let ids = parse("2-Belvedere-Drive-Waterford/6010625\n6721798\ndaft-6710820\n\n");

        assert_eq!(ids.len(), 3);
        assert!(ids.contains("6010625"));
        assert!(ids.contains("6721798"));
        assert!(ids.contains("6710820"));
    }

    #[test]
    fn appends_and_reloads_ids() -> Result<(), String> {
        let directory =
            tempfile::tempdir().map_err(|error| format!("failed to create temp dir: {error}"))?;
        let path = directory.path().join("sent_ids.txt");

        assert!(load(&path).is_empty());
        append(&path, &["1".to_string(), "2".to_string()])?;
        append(&path, &["3".to_string()])?;

        let ids = load(&path);
        assert_eq!(ids.len(), 3);
        assert!(ids.contains("3"));

        Ok(())
    }
}
