use anyhow::{Context as _, Result, bail};
use nanoserde::DeJson;

use crate::listing::Coordinates;

pub(crate) const ENV_VAR: &str = "PLACES_OF_INTEREST";

/// A named location from `PLACES_OF_INTEREST`. (nanoserde-derive panics on a
/// `pub(crate)` struct, so this is `pub`.)
#[derive(Debug, Clone, DeJson)]
pub struct Place {
    pub(crate) name: String,
    pub(crate) latitude: f64,
    pub(crate) longitude: f64,
}

/// Parses and validates the `PLACES_OF_INTEREST` JSON array.
pub(crate) fn parse(value: &str) -> Result<Vec<Place>> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(Vec::new());
    }

    let places: Vec<Place> =
        Vec::deserialize_json(value).with_context(|| format!("Failed to parse {ENV_VAR} JSON"))?;

    for place in &places {
        if place.name.trim().is_empty() {
            bail!("{ENV_VAR} contains a place with an empty name");
        }
        if !(-90.0..=90.0).contains(&place.latitude) || !(-180.0..=180.0).contains(&place.longitude)
        {
            bail!(
                "{ENV_VAR} contains invalid coordinates for '{}'",
                place.name
            );
        }
    }

    Ok(places)
}

pub(crate) fn distance_km(from: Coordinates, to: &Place) -> f64 {
    const EARTH_RADIUS_KM: f64 = 6371.0088;

    let from_latitude = from.latitude.to_radians();
    let to_latitude = to.latitude.to_radians();
    let latitude_delta = (to.latitude - from.latitude).to_radians();
    let longitude_delta = (to.longitude - from.longitude).to_radians();

    let latitude_sin = (latitude_delta / 2.0).sin();
    let longitude_sin = (longitude_delta / 2.0).sin();
    let a = (from_latitude.cos() * to_latitude.cos())
        .mul_add(longitude_sin.powi(2), latitude_sin.powi(2))
        .clamp(0.0, 1.0);
    let central_angle = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());

    EARTH_RADIUS_KM * central_angle
}

pub(crate) fn format_distance(distance_km: f64) -> String {
    if distance_km < 1.0 {
        format!("{:.0} m", distance_km * 1000.0)
    } else if distance_km < 10.0 {
        format!("{distance_km:.1} km")
    } else {
        format!("{distance_km:.0} km")
    }
}

#[cfg(test)]
mod tests {
    use anyhow::{Result, bail};

    use super::{Place, distance_km, format_distance, parse};
    use crate::listing::Coordinates;
    #[test]
    fn parses_places_and_distances() -> Result<()> {
        let places = parse(
            r#"[{"name":"Work","latitude":52.244898,"longitude":-7.159315},{"name":"Geoff's","latitude":52.258076,"longitude":-7.111913}]"#,
        )?;

        assert_eq!(places.len(), 2);
        assert_eq!(
            places.first().map(|place| place.name.as_str()),
            Some("Work")
        );
        assert_eq!(
            places.get(1).map(|place| place.name.as_str()),
            Some("Geoff's")
        );

        let Some(work) = places.first() else {
            bail!("expected Work place");
        };
        let distance = distance_km(
            Coordinates {
                latitude: 52.244_898,
                longitude: -7.159_315,
            },
            work,
        );
        assert!(distance.abs() < 0.001);
        assert_eq!(format_distance(0.0), "0 m");
        assert_eq!(format_distance(1.25), "1.2 km");

        Ok(())
    }

    #[test]
    fn rejects_invalid_coordinates() {
        assert!(parse(r#"[{"name":"Bad","latitude":91,"longitude":0}]"#).is_err());
    }

    #[test]
    fn formats_short_distances_as_metres() {
        let place = Place {
            name: "Near".to_string(),
            latitude: 52.245_898,
            longitude: -7.159_315,
        };
        let distance = distance_km(
            Coordinates {
                latitude: 52.244_898,
                longitude: -7.159_315,
            },
            &place,
        );
        assert!(distance < 1.0);
        assert!(format_distance(distance).ends_with(" m"));
    }
}
