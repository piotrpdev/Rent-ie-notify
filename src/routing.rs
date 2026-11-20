use anyhow::{Context as _, Result, bail};
use std::collections::HashMap;

use nanoserde::{DeJson, SerJson};

use crate::delay;
use crate::listing::Listing;
use crate::places::Place;

const ORS_MATRIX_URL: &str = "https://api.openrouteservice.org/v2/matrix/driving-car";
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

fn fetch_batch(listings: &[&Listing], places: &[Place], api_key: &str) -> Result<RouteMap> {
    let mut locations = Vec::with_capacity(listings.len() + places.len());

    for listing in listings {
        let Some(coordinates) = listing.coordinates else {
            continue;
        };
        locations.push([coordinates.longitude, coordinates.latitude]);
    }
    for place in places {
        locations.push([place.longitude, place.latitude]);
    }

    let request = MatrixRequest {
        locations,
        sources: (0..listings.len()).collect(),
        destinations: (listings.len()..listings.len() + places.len()).collect(),
        metrics: vec!["duration".to_string(), "distance".to_string()],
    };
    let body = request.serialize_json();

    let response = reqwest::blocking::Client::new()
        .post(ORS_MATRIX_URL)
        .header("Authorization", api_key)
        .header("Content-Type", "application/json")
        .body(body)
        .send()
        .map_err(crate::http::without_url)
        .context("OpenRouteService request failed")?;

    let status = response.status().as_u16();
    if status != 200 {
        bail!("OpenRouteService returned status {status}");
    }

    let json = response
        .text()
        .map_err(crate::http::without_url)
        .context("Failed to read OpenRouteService response")?;
    let data: MatrixResponse = MatrixResponse::deserialize_json(&json)
        .context("Failed to parse OpenRouteService response")?;

    let mut routes = RouteMap::new();

    for (row_index, listing) in listings.iter().enumerate() {
        let durations = data.durations.get(row_index);
        let distances = data.distances.get(row_index);
        let mut measurements = Vec::with_capacity(places.len());

        for place_index in 0..places.len() {
            let duration = durations
                .and_then(|row| row.get(place_index))
                .copied()
                .flatten();
            let distance = distances
                .and_then(|row| row.get(place_index))
                .copied()
                .flatten();

            measurements.push(match (duration, distance) {
                (Some(duration_seconds), Some(distance_meters)) => Some(RouteMeasurement {
                    duration_seconds,
                    distance_meters,
                }),
                _ => None,
            });
        }

        routes.insert(listing.id.clone(), measurements);
    }

    Ok(routes)
}

/// Fetches drive times/distances for new listings. Any batch failure is logged
/// and left absent from the map, so the Slack layer can fall back to straight-line
/// distances instead of failing the whole run.
pub(crate) fn fetch_routes(listings: &[Listing], places: &[Place], api_key: &str) -> RouteMap {
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

    let origins: Vec<&Listing> = listings
        .iter()
        .filter(|listing| listing.coordinates.is_some())
        .collect();
    let batch_count = origins.len().div_ceil(max_origins);

    for (batch_index, batch) in origins.chunks(max_origins).enumerate() {
        match fetch_batch(batch, places, api_key) {
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
    use super::format_duration;

    #[test]
    fn formats_drive_durations() {
        assert_eq!(format_duration(30.0), "<1 min");
        assert_eq!(format_duration(90.0), "2 min");
        assert_eq!(format_duration(3900.0), "1 h 5 min");
    }
}
