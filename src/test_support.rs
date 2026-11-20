//! Shared helpers for unit tests.

use std::thread::JoinHandle;

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
/// `statuses` (in order) and returns its URL plus a handle yielding the
/// received request bodies.
pub(crate) fn run_sink(statuses: Vec<u16>) -> Result<(String, JoinHandle<Vec<String>>), String> {
    let server = tiny_http::Server::http("127.0.0.1:0").map_err(|error| error.to_string())?;
    let port = server
        .server_addr()
        .to_ip()
        .map(|address| address.port())
        .ok_or("sink has no IP address")?;

    let handle = std::thread::spawn(move || {
        let mut bodies = Vec::new();
        for status in statuses {
            let Ok(mut request) = server.recv() else {
                break;
            };
            let mut body = String::new();
            let _ = request.as_reader().read_to_string(&mut body);
            bodies.push(body);
            let _ = request.respond(
                tiny_http::Response::from_string("invalid_blocks").with_status_code(status),
            );
        }
        bodies
    });

    Ok((format!("http://127.0.0.1:{port}/"), handle))
}
