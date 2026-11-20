//! `sent_ids.txt`: the IDs of every listing already posted to Slack.

use std::{
    collections::HashSet,
    fs::{File, OpenOptions, TryLockError},
    io::{ErrorKind, Write},
    path::Path,
};

pub(crate) const SENT_IDS_FILENAME: &str = "sent_ids.txt";
pub(crate) const LOCK_FILENAME: &str = "sent_ids.txt.lock";

/// Exclusive lock held for a whole run, so two runs in the same directory
/// can't both read the same state and post the same listings. Released when
/// dropped or when the process exits.
#[derive(Debug)]
pub(crate) struct RunLock {
    _file: File,
}

/// Takes the run lock at `path`, failing immediately if another run holds it.
pub(crate) fn lock(path: &Path) -> Result<RunLock, String> {
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)
        .map_err(|error| format!("Failed to open {}: {error}", path.display()))?;

    match file.try_lock() {
        Ok(()) => Ok(RunLock { _file: file }),
        Err(TryLockError::WouldBlock) => Err(format!(
            "Another run is in progress ({} is locked)",
            path.display()
        )),
        Err(TryLockError::Error(error)) => {
            Err(format!("Failed to lock {}: {error}", path.display()))
        }
    }
}

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

/// Loads previously sent IDs. A missing file means nothing has been sent yet;
/// any other read error is returned, because treating an unreadable file as
/// empty would re-send every listing.
pub(crate) fn load(path: &Path) -> Result<HashSet<String>, String> {
    match std::fs::read_to_string(path) {
        Ok(content) => Ok(parse(&content)),
        Err(error) if error.kind() == ErrorKind::NotFound => {
            eprintln!(
                "{} not found, assuming no listings have been sent yet.",
                path.display()
            );
            Ok(HashSet::new())
        }
        Err(error) => Err(format!("Failed to read {}: {error}", path.display())),
    }
}

/// Appends `ids`, one per line, creating the file if needed, and syncs it to disk.
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
        .map_err(|error| format!("Failed to write {}: {error}", path.display()))?;
    // These IDs were just posted to Slack; flush them to disk so a power loss
    // (e.g. a Raspberry Pi on an SD card) can't drop them and cause re-posts.
    file.sync_data()
        .map_err(|error| format!("Failed to sync {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::{append, load, lock, parse};

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

        assert!(load(&path)?.is_empty());
        append(&path, &["1".to_string(), "2".to_string()])?;
        append(&path, &["3".to_string()])?;

        let ids = load(&path)?;
        assert_eq!(ids.len(), 3);
        assert!(ids.contains("3"));

        Ok(())
    }

    #[test]
    fn fails_on_unreadable_state_instead_of_starting_fresh() -> Result<(), String> {
        let directory =
            tempfile::tempdir().map_err(|error| format!("failed to create temp dir: {error}"))?;
        // A directory in place of the file is unreadable but exists.
        assert!(load(directory.path()).is_err());

        let path = directory.path().join("sent_ids.txt");
        std::fs::write(&path, [0xff, 0xfe])
            .map_err(|error| format!("failed to write fixture: {error}"))?;
        assert!(load(&path).is_err());

        Ok(())
    }

    #[test]
    fn allows_only_one_run_lock_at_a_time() -> Result<(), String> {
        let directory =
            tempfile::tempdir().map_err(|error| format!("failed to create temp dir: {error}"))?;
        let path = directory.path().join("sent_ids.txt.lock");

        let first = lock(&path)?;
        assert!(lock(&path).is_err_and(|error| error.contains("in progress")));
        drop(first);
        lock(&path)?;

        Ok(())
    }
}
