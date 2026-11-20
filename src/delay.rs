use std::{
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const BASE_DELAY: Duration = Duration::from_secs(1);
const MAX_JITTER_MILLIS: u64 = 500;

/// Sleeps for one second plus up to half a second of jitter.
pub(crate) fn sleep() {
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or_default(|duration| u64::from(duration.subsec_nanos()));
    let jitter = Duration::from_millis(seed % (MAX_JITTER_MILLIS + 1));

    thread::sleep(BASE_DELAY + jitter);
}
