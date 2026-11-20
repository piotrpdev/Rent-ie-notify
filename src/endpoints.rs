//! Base URLs of the external APIs. Production uses [`Endpoints::default`];
//! tests point them at local servers. Public listing links always use the
//! real sites.

#[derive(Debug, Clone)]
pub(crate) struct Endpoints {
    /// Origin for rent.ie `_next/data` search/property requests and the homepage.
    pub(crate) rent_ie: String,
    /// daft.ie gateway listings endpoint.
    pub(crate) daft_ie_api: String,
    /// Origin for property.ie `_next/data` search requests.
    pub(crate) property_ie: String,
    /// ORS (openrouteservice.org) `driving-car` matrix endpoint.
    pub(crate) ors_matrix: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            rent_ie: "https://www.rent.ie".to_string(),
            daft_ie_api: "https://gateway.daft.ie/api/v2/ads/listings".to_string(),
            property_ie: "https://www.property.ie".to_string(),
            ors_matrix: "https://api.openrouteservice.org/v2/matrix/driving-car".to_string(),
        }
    }
}
