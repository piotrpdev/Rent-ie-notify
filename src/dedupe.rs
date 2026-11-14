use crate::listing::Listing;

fn are_duplicates(left: &Listing, right: &Listing) -> bool {
    left.id == right.id
        || (left.source != right.source
            && left
                .image_keys
                .iter()
                .any(|key| right.image_keys.contains(key)))
}

/// Groups listings that share a numeric ID or any decoded media key.
///
/// Listings are kept in input order, so the first source configured wins when
/// representatives are chosen.
pub(crate) fn representative(group: &[Listing]) -> Option<Listing> {
    let mut representative = group.first()?.clone();

    if representative.coordinates.is_none() {
        representative.coordinates = group.iter().find_map(|candidate| candidate.coordinates);
    }

    Some(representative)
}

pub(crate) fn group_listings(mut listings: Vec<Listing>) -> Vec<Vec<Listing>> {
    let mut groups = Vec::new();

    while !listings.is_empty() {
        let mut group = vec![listings.remove(0)];
        let mut index = 0;

        while index < listings.len() {
            let duplicate = listings.get(index).is_some_and(|candidate| {
                group.iter().any(|member| are_duplicates(member, candidate))
            });

            if duplicate {
                group.push(listings.remove(index));
                index = 0;
            } else {
                index += 1;
            }
        }

        groups.push(group);
    }

    groups
}

#[cfg(test)]
mod tests {
    use super::{group_listings, representative};
    use crate::listing::{Listing, ListingSource};
    use crate::{daft_ie, rent_ie};

    fn listing(id: &str, image_keys: &[&str], source: ListingSource) -> Listing {
        Listing {
            id: id.to_string(),
            source,
            link: String::new(),
            img_url: String::new(),
            image_keys: image_keys.iter().map(ToString::to_string).collect(),
            coordinates: None,
            address: String::new(),
            price: String::new(),
            date: String::new(),
            description: String::new(),
        }
    }

    #[test]
    fn groups_by_shared_id_or_image_key() {
        let groups = group_listings(vec![
            listing("1", &["a"], ListingSource::RentIe),
            listing("2", &["b"], ListingSource::RentIe),
            listing("3", &["a"], ListingSource::DaftIe),
            listing("2", &["c"], ListingSource::DaftIe),
        ]);

        assert_eq!(groups.len(), 2);
        assert_eq!(groups.first().map(Vec::len), Some(2));
        assert_eq!(groups.get(1).map(Vec::len), Some(2));
    }

    #[test]
    fn keeps_unmatched_listings_separate() {
        let groups = group_listings(vec![
            listing("1", &["a"], ListingSource::RentIe),
            listing("2", &["b"], ListingSource::DaftIe),
            listing("3", &[], ListingSource::RentIe),
        ]);

        assert_eq!(groups.len(), 3);
    }

    #[test]
    fn does_not_group_same_source_image_reuse() {
        let groups = group_listings(vec![
            listing("1", &["a"], ListingSource::RentIe),
            listing("2", &["a"], ListingSource::RentIe),
        ]);

        assert_eq!(groups.len(), 2);
    }

    #[test]
    fn groups_transitively() {
        let groups = group_listings(vec![
            listing("1", &["a"], ListingSource::RentIe),
            listing("2", &["a", "b"], ListingSource::DaftIe),
            listing("3", &["b"], ListingSource::RentIe),
        ]);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups.first().map(Vec::len), Some(3));
    }

    #[test]
    #[ignore = "uses the local archives/ fixtures"]
    fn groups_archived_cross_site_duplicates() -> Result<(), String> {
        let rent_json = std::fs::read_to_string(
            "archives/rentie/search_waterford_city_centre_rent_0-550_page_1.json",
        )
        .map_err(|error| format!("failed to read rent.ie fixture: {error}"))?;
        let daft_json = std::fs::read_to_string(
            "archives/daftie/sharing_waterford_city_rent_0-550_page_1.json",
        )
        .map_err(|error| format!("failed to read daft.ie fixture: {error}"))?;

        let (rent_listings, _) = rent_ie::extract_page_from_json(&rent_json)?;
        let (daft_listings, _) = daft_ie::extract_page_from_json(&daft_json)?;
        assert_eq!(rent_listings.len(), 17);
        assert_eq!(daft_listings.len(), 17);

        let mut listings = rent_listings;
        listings.extend(daft_listings);
        let groups = group_listings(listings);

        assert_eq!(groups.len(), 17);
        assert!(groups.iter().all(|group| group.len() == 2));
        assert!(
            groups
                .iter()
                .all(|group| group.iter().all(|listing| !listing.image_keys.is_empty()))
        );
        for group in &groups {
            let Some(first) = group.first() else {
                continue;
            };
            assert!(group.iter().skip(1).all(|other| {
                first
                    .image_keys
                    .iter()
                    .any(|key| other.image_keys.contains(key))
            }));

            let Some(representative) = representative(group) else {
                continue;
            };
            if group.iter().any(|listing| listing.coordinates.is_some()) {
                assert!(representative.coordinates.is_some());
            }
        }

        Ok(())
    }
}
