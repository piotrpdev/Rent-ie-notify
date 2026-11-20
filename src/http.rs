use anyhow::{Context as _, Result, bail};
use reqwest::blocking::RequestBuilder;

use crate::{delay, listing::Listing};

/// Result of parsing one search results page: its listings and the cursor
/// (page number or offset) of the next page, if any.
pub(crate) type Page = (Vec<Listing>, Option<u32>);

/// Removes the request URL from a reqwest error, since it can be a secret
/// (Slack webhook, rent.ie search URL). Wrap reqwest errors with this before
/// adding context; the cause chain (e.g. "connection refused") is kept.
pub(crate) fn without_url(error: reqwest::Error) -> anyhow::Error {
    anyhow::Error::new(error.without_url())
}

/// Sends `request` and returns the body of a 2xx response. `what` names the
/// resource in error messages, e.g. "rent.ie search page".
/// Errors never include the request URL (see [`without_url`]).
pub(crate) fn send_text(request: RequestBuilder, what: &str) -> Result<String> {
    let response = request
        .send()
        .map_err(without_url)
        .with_context(|| format!("Failed to fetch {what}"))?;

    let status = response.status();
    if !status.is_success() {
        let hint = if status.as_u16() == 403 {
            " (likely Cloudflare bot protection; try from a residential IP?)"
        } else {
            ""
        };
        bail!(
            "Failed to fetch {what}: received status code {}{hint}",
            status.as_u16()
        );
    }

    response
        .text()
        .map_err(without_url)
        .with_context(|| format!("Failed to read {what}"))
}

/// Upper bound on pages fetched per source, as a guard against APIs that
/// ignore the page parameter. 50 daft.ie pages of 50 is 2,500 listings.
const MAX_PAGES: usize = 50;

/// Fetches pages sequentially until `fetch_page` reports no next cursor,
/// sleeping between requests and logging progress as `source`. The first call
/// receives `None`.
///
/// Stops early, keeping what was fetched, if the cursor doesn't advance or
/// after [`MAX_PAGES`] pages, so a misbehaving API can't loop forever (which
/// would also block every later systemd timer run).
pub(crate) fn paginate(
    source: &str,
    mut fetch_page: impl FnMut(Option<u32>) -> Result<Page>,
) -> Result<Vec<Listing>> {
    let mut listings = Vec::new();
    let mut cursor = None;

    for page_number in 1..=MAX_PAGES {
        println!("Fetching {source} results page {page_number}...");
        let (page, next) = fetch_page(cursor)?;
        listings.extend(page);

        let Some(next) = next else {
            println!(
                "Fetched {} {source} listings from {page_number} page(s).",
                listings.len()
            );
            return Ok(listings);
        };
        if cursor.is_some_and(|cursor| next <= cursor) {
            eprintln!("{source} pagination cursor did not advance ({next}); stopping early.");
            return Ok(listings);
        }
        delay::sleep();
        cursor = Some(next);
    }

    eprintln!("Stopped {source} after {MAX_PAGES} pages; later results were not fetched.");
    Ok(listings)
}

#[cfg(test)]
mod tests {
    use anyhow::Result;

    use super::{MAX_PAGES, paginate};

    #[test]
    fn follows_cursors_until_the_last_page() -> Result<()> {
        let mut requested = Vec::new();
        paginate("test", |cursor| {
            requested.push(cursor);
            Ok((
                Vec::new(),
                cursor.map_or(Some(2), |page| (page < 3).then_some(page + 1)),
            ))
        })?;
        assert_eq!(requested, vec![None, Some(2), Some(3)]);
        Ok(())
    }

    #[test]
    fn stops_when_the_cursor_does_not_advance() -> Result<()> {
        let mut calls = 0;
        paginate("test", |_| {
            calls += 1;
            Ok((Vec::new(), Some(2)))
        })?;
        assert_eq!(calls, 2);
        Ok(())
    }

    #[test]
    fn stops_after_the_page_limit() -> Result<()> {
        let mut calls: u32 = 0;
        paginate("test", |_| {
            calls += 1;
            Ok((Vec::new(), Some(calls + 1)))
        })?;
        assert_eq!(usize::try_from(calls).ok(), Some(MAX_PAGES));
        Ok(())
    }
}
