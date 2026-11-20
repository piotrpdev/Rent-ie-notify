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

/// Fetches pages sequentially until `fetch_page` reports no next cursor,
/// sleeping between requests. The first call receives `None`.
pub(crate) fn paginate(
    mut fetch_page: impl FnMut(Option<u32>) -> Result<Page, String>,
) -> Result<Vec<Listing>, String> {
    let mut listings = Vec::new();
    let mut cursor = None;

    loop {
        let (page, next) = fetch_page(cursor)?;
        listings.extend(page);

        let Some(next) = next else {
            return Ok(listings);
        };
        delay::sleep();
        cursor = Some(next);
    }
}
