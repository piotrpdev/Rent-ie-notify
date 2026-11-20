//! Shared helpers for unit tests.

use std::thread::JoinHandle;

use anyhow::{Context as _, Result, anyhow};

use crate::listing::{Listing, ListingDetails, ListingSource, Section};

/// A listing with the given ID and source and otherwise empty fields.
pub(crate) fn listing(id: &str, source: ListingSource) -> Listing {
    Listing {
        id: id.to_string(),
        source,
        link: format!("https://example.com/{id}"),
        img_url: String::new(),
        image_keys: Vec::new(),
        coordinates: None,
        address: format!("Address {id}"),
        price: "€500 per month".to_string(),
        date: String::new(),
        description: "House".to_string(),
        bedrooms: None,
        details: ListingDetails::default(),
        section: Section::HousesToLet,
    }
}

/// A request received by [`run_server`].
#[derive(Debug)]
pub(crate) struct Recorded {
    /// Path and query, e.g. `/_next/data/x/search.json?page=2`.
    pub(crate) url: String,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) body: String,
}

impl Recorded {
    pub(crate) fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

/// Starts a local HTTP server that answers one request per entry in
/// `responses` (status and body, in order, each with `Retry-After: 0`) and
/// returns its base URL (no trailing slash) plus a handle yielding the
/// received requests.
pub(crate) fn run_server(
    responses: Vec<(u16, String)>,
) -> Result<(String, JoinHandle<Vec<Recorded>>)> {
    let server = tiny_http::Server::http("127.0.0.1:0").map_err(|error| anyhow!("{error}"))?;
    let port = server
        .server_addr()
        .to_ip()
        .map(|address| address.port())
        .context("server has no IP address")?;

    let handle = std::thread::spawn(move || {
        let mut received = Vec::new();
        for (status, body) in responses {
            let Ok(mut request) = server.recv() else {
                break;
            };
            let mut request_body = String::new();
            let _ = request.as_reader().read_to_string(&mut request_body);
            received.push(Recorded {
                url: request.url().to_string(),
                headers: request
                    .headers()
                    .iter()
                    .map(|header| (header.field.to_string(), header.value.to_string()))
                    .collect(),
                body: request_body,
            });
            let mut response = tiny_http::Response::from_string(body).with_status_code(status);
            if let Ok(header) = tiny_http::Header::from_bytes("Retry-After", "0") {
                response.add_header(header);
            }
            let _ = request.respond(response);
        }
        received
    });

    Ok((format!("http://127.0.0.1:{port}"), handle))
}

/// A Slack-like sink: answers one request per status in `statuses` with body
/// `invalid_blocks` and returns its URL plus a handle yielding the bodies.
pub(crate) fn run_sink(statuses: Vec<u16>) -> Result<(String, JoinHandle<Vec<String>>)> {
    let responses = statuses
        .into_iter()
        .map(|status| (status, "invalid_blocks".to_string()))
        .collect();
    let (url, handle) = run_server(responses)?;
    let bodies = std::thread::spawn(move || {
        handle
            .join()
            .map_or_default(|received| received.into_iter().map(|request| request.body).collect())
    });
    Ok((format!("{url}/"), bodies))
}
