use reqwest::blocking::RequestBuilder;

use crate::{delay, listing::Listing};

/// Result of parsing one search results page: its listings and the cursor
/// (page number or offset) of the next page, if any.
pub(crate) type Page = (Vec<Listing>, Option<u32>);

/// Formats a reqwest error with its cause chain but without the request URL,
/// which can be a secret (Slack webhook, rent.ie search URL).
pub(crate) fn describe_error(error: reqwest::Error) -> String {
    let error = error.without_url();
    let mut message = error.to_string();
    let mut source = std::error::Error::source(&error);
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    message
}

/// Sends `request` and returns the body of a 2xx response. `what` names the
/// resource in error messages, e.g. "rent.ie search page".
/// Error messages never include the request URL (see [`describe_error`]).
pub(crate) fn send_text(request: RequestBuilder, what: &str) -> Result<String, String> {
    let response = request
        .send()
        .map_err(|error| format!("Failed to fetch {what}: {}", describe_error(error)))?;

    let status = response.status();
    if !status.is_success() {
        let hint = if status.as_u16() == 403 {
            " (likely Cloudflare bot protection; try from a residential IP?)"
        } else {
            ""
        };
        return Err(format!(
            "Failed to fetch {what}: received status code {}{hint}",
            status.as_u16()
        ));
    }

    response
        .text()
        .map_err(|error| format!("Failed to read {what}: {}", describe_error(error)))
}

/// Upper bound on pages fetched per source, as a guard against APIs that
/// ignore the page parameter. 50 daft.ie pages of 50 is 2,500 listings.
const MAX_PAGES: usize = 50;

/// Fetches pages sequentially until `fetch_page` reports no next cursor,
/// sleeping between requests. The first call receives `None`.
///
/// Stops early, keeping what was fetched, if the cursor doesn't advance or
/// after [`MAX_PAGES`] pages, so a misbehaving API can't loop forever (which
/// would also block every later systemd timer run).
pub(crate) fn paginate(
    fetch_page: impl FnMut(Option<u32>) -> Result<Page, String>,
) -> Result<Vec<Listing>, String> {
    paginate_with(fetch_page, delay::sleep)
}

fn paginate_with(
    mut fetch_page: impl FnMut(Option<u32>) -> Result<Page, String>,
    mut sleep: impl FnMut(),
) -> Result<Vec<Listing>, String> {
    let mut listings = Vec::new();
    let mut cursor = None;

    for _ in 0..MAX_PAGES {
        let (page, next) = fetch_page(cursor)?;
        listings.extend(page);

        let Some(next) = next else {
            return Ok(listings);
        };
        if cursor.is_some_and(|cursor| next <= cursor) {
            eprintln!("Pagination cursor did not advance ({next}); stopping early.");
            return Ok(listings);
        }
        sleep();
        cursor = Some(next);
    }

    eprintln!("Stopped after {MAX_PAGES} pages; later results were not fetched.");
    Ok(listings)
}

#[cfg(test)]
mod tests {
    use super::{MAX_PAGES, paginate_with};

    #[test]
    fn follows_cursors_until_the_last_page() -> Result<(), String> {
        let mut requested = Vec::new();
        paginate_with(
            |cursor| {
                requested.push(cursor);
                Ok((
                    Vec::new(),
                    cursor.map_or(Some(2), |page| (page < 3).then_some(page + 1)),
                ))
            },
            || {},
        )?;
        assert_eq!(requested, vec![None, Some(2), Some(3)]);
        Ok(())
    }

    #[test]
    fn stops_when_the_cursor_does_not_advance() -> Result<(), String> {
        let mut calls = 0;
        paginate_with(
            |_| {
                calls += 1;
                Ok((Vec::new(), Some(2)))
            },
            || {},
        )?;
        assert_eq!(calls, 2);
        Ok(())
    }

    #[test]
    fn stops_after_the_page_limit() -> Result<(), String> {
        let mut calls: u32 = 0;
        paginate_with(
            |_| {
                calls += 1;
                Ok((Vec::new(), Some(calls + 1)))
            },
            || {},
        )?;
        assert_eq!(usize::try_from(calls).ok(), Some(MAX_PAGES));
        Ok(())
    }
}
