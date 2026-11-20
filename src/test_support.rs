//! Shared helpers for unit tests.

use std::thread::JoinHandle;

use anyhow::{Context as _, Result, anyhow};

use crate::listing::{Listing, ListingDetails, ListingSource};

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
        detail_property_type: "houses-to-let".to_string(),
    }
}

/// Starts a local HTTP server that answers one request per entry in
/// `statuses` (in order, each with `Retry-After: 0` and body `invalid_blocks`) and returns its URL plus a handle yielding the
/// received request bodies.
pub(crate) fn run_sink(statuses: Vec<u16>) -> Result<(String, JoinHandle<Vec<String>>)> {
    let server = tiny_http::Server::http("127.0.0.1:0").map_err(|error| anyhow!("{error}"))?;
    let port = server
        .server_addr()
        .to_ip()
        .map(|address| address.port())
        .context("sink has no IP address")?;

    let handle = std::thread::spawn(move || {
        let mut bodies = Vec::new();
        for status in statuses {
            let Ok(mut request) = server.recv() else {
                break;
            };
            let mut body = String::new();
            let _ = request.as_reader().read_to_string(&mut body);
            bodies.push(body);
            let mut response =
                tiny_http::Response::from_string("invalid_blocks").with_status_code(status);
            if let Ok(header) = tiny_http::Header::from_bytes("Retry-After", "0") {
                response.add_header(header);
            }
            let _ = request.respond(response);
        }
        bodies
    });

    Ok((format!("http://127.0.0.1:{port}/"), handle))
}
