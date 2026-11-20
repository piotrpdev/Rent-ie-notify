use anyhow::{Context as _, Result};
use std::collections::HashMap;

use nanoserde::{DeJson, SerJson};
use reqwest::blocking::Client;

use crate::delay;
use crate::http;
use crate::listing::{Coordinates, Listing};
use crate::places::Place;

const MAX_MATRIX_LOCATIONS: usize = 50;
const MAX_ORIGINS_PER_BATCH: usize = 40;

pub(crate) type RouteMap = HashMap<String, Vec<Option<RouteMeasurement>>>;

#[derive(Debug, Clone, Copy)]
pub(crate) struct RouteMeasurement {
    pub(crate) duration_seconds: f64,
    pub(crate) distance_meters: f64,
}

#[derive(Debug, SerJson)]
struct MatrixRequest {
    locations: Vec<[f64; 2]>,
    sources: Vec<usize>,
    destinations: Vec<usize>,
    metrics: Vec<String>,
}

#[derive(Debug, DeJson)]
struct MatrixResponse {
    durations: Vec<Vec<Option<f64>>>,
    distances: Vec<Vec<Option<f64>>>,
}

/// A listing ID and its coordinates, i.e. one matrix origin.
type Origin<'a> = (&'a str, Coordinates);

fn fetch_batch(
    client: &Client,
    matrix_url: &str,
    origins: &[Origin<'_>],
    places: &[Place],
    api_key: &str,
) -> Result<RouteMap> {
    // ORS takes [longitude, latitude]: origins first, then destinations.
    let locations = origins
        .iter()
        .map(|(_, coordinates)| [coordinates.longitude, coordinates.latitude])
        .chain(places.iter().map(|place| [place.longitude, place.latitude]))
        .collect();
    let request = MatrixRequest {
        locations,
        sources: (0..origins.len()).collect(),
        destinations: (origins.len()..origins.len() + places.len()).collect(),
        metrics: vec!["duration".to_string(), "distance".to_string()],
    };

    let json = http::send_text(
        client
            .post(matrix_url)
            .header("Authorization", api_key)
            .header("Content-Type", "application/json")
            .body(request.serialize_json()),
        "OpenRouteService matrix",
    )?;
    let data = MatrixResponse::deserialize_json(&json)
        .context("Failed to parse OpenRouteService response")?;

    let measurement = |row: Option<&Vec<Option<f64>>>, place_index: usize| {
        row.and_then(|row| row.get(place_index)).copied().flatten()
    };
    let routes = origins
        .iter()
        .enumerate()
        .map(|(row_index, (id, _))| {
            let durations = data.durations.get(row_index);
            let distances = data.distances.get(row_index);
            let measurements = (0..places.len())
                .map(|place_index| {
                    Some(RouteMeasurement {
                        duration_seconds: measurement(durations, place_index)?,
                        distance_meters: measurement(distances, place_index)?,
                    })
                })
                .collect();
            ((*id).to_string(), measurements)
        })
        .collect();

    Ok(routes)
}

/// Fetches drive times/distances for new listings. Any batch failure is logged
/// and left absent from the map, so the Slack layer can fall back to straight-line
/// distances instead of failing the whole run.
pub(crate) fn fetch_routes(
    matrix_url: &str,
    listings: &[Listing],
    places: &[Place],
    api_key: &str,
) -> RouteMap {
    let mut routes = RouteMap::new();

    if places.is_empty() || api_key.trim().is_empty() {
        return routes;
    }

    let max_origins = MAX_ORIGINS_PER_BATCH.min(MAX_MATRIX_LOCATIONS.saturating_sub(places.len()));
    if max_origins == 0 {
        eprintln!(
            "Too many places for OpenRouteService matrix limits; using straight-line distances."
        );
        return routes;
    }

    let origins: Vec<Origin<'_>> = listings
        .iter()
        .filter_map(|listing| Some((listing.id.as_str(), listing.coordinates?)))
        .collect();
    let client = Client::new();
    let batch_count = origins.len().div_ceil(max_origins);

    for (batch_index, batch) in origins.chunks(max_origins).enumerate() {
        match fetch_batch(&client, matrix_url, batch, places, api_key) {
            Ok(batch_routes) => routes.extend(batch_routes),
            Err(error) => eprintln!(
                "Failed to fetch drive times from OpenRouteService: {error:#}; using straight-line fallback for this batch."
            ),
        }

        if batch_index + 1 < batch_count {
            delay::sleep();
        }
    }

    if !routes.is_empty() {
        println!("Fetched drive times for {} listings.", routes.len());
    }

    routes
}

pub(crate) fn format_duration(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "?".to_string();
    }

    if seconds < 60.0 {
        return "<1 min".to_string();
    }

    let total_minutes = (seconds / 60.0).round();
    if total_minutes < 60.0 {
        format!("{total_minutes:.0} min")
    } else {
        let hours = (total_minutes / 60.0).floor();
        let minutes = hours.mul_add(-60.0, total_minutes);
        format!("{hours:.0} h {minutes:.0} min")
    }
}

#[cfg(test)]
mod tests {
    use anyhow::{Result, anyhow};

    use super::{fetch_routes, format_duration};
    use crate::listing::{Coordinates, Listing, ListingSource};
    use crate::places::Place;
    use crate::test_support::{listing, run_server};

    fn inputs() -> (Vec<Listing>, Vec<Place>) {
        let with_coordinates = Listing {
            coordinates: Some(Coordinates {
                latitude: 52.25,
                longitude: -7.11,
            }),
            ..listing("1", ListingSource::Daft)
        };
        let places = vec![Place {
            name: "Work".to_string(),
            latitude: 52.24,
            longitude: -7.15,
        }];
        (
            vec![with_coordinates, listing("2", ListingSource::Rent)],
            places,
        )
    }

    #[test]
    fn formats_drive_durations() {
        assert_eq!(format_duration(30.0), "<1 min");
        assert_eq!(format_duration(90.0), "2 min");
        assert_eq!(format_duration(3900.0), "1 h 5 min");
    }

    #[test]
    fn fetches_drive_times_for_listings_with_coordinates() -> Result<()> {
        let (url, server) = run_server(vec![(
            200,
            r#"{"durations":[[480.0]],"distances":[[2400.0]]}"#.to_string(),
        )])?;
        let (listings, places) = inputs();

        let routes = fetch_routes(&format!("{url}/matrix"), &listings, &places, "key");

        let measurement = routes
            .get("1")
            .and_then(|row| row.first().copied().flatten())
            .ok_or_else(|| anyhow!("expected a route for listing 1"))?;
        assert!((measurement.duration_seconds - 480.0).abs() < f64::EPSILON);
        assert!(!routes.contains_key("2"));

        let requests = server.join().map_err(|_| anyhow!("server panicked"))?;
        let request = requests
            .first()
            .ok_or_else(|| anyhow!("expected a request"))?;
        assert_eq!(request.header("Authorization"), Some("key"));
        assert!(request.body.contains(r#""sources":[0],"destinations":[1]"#));
        Ok(())
    }

    #[test]
    fn returns_no_routes_when_the_api_fails() -> Result<()> {
        let (url, server) = run_server(vec![(500, String::new())])?;
        let (listings, places) = inputs();

        let routes = fetch_routes(&format!("{url}/matrix"), &listings, &places, "key");

        assert!(routes.is_empty());
        server.join().map_err(|_| anyhow!("server panicked"))?;
        Ok(())
    }
}
