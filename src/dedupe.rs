use std::collections::HashMap;

use crate::listing::{Listing, ListingSource};

/// Prefers the daft.ie listing as the group representative, then falls back to
/// the first listing in the group.
pub(crate) fn representative(group: &[Listing]) -> Option<Listing> {
    let mut representative = group
        .iter()
        .find(|listing| listing.source == ListingSource::Daft)
        .or_else(|| group.first())?
        .clone();

    if representative.coordinates.is_none() {
        representative.coordinates = group.iter().find_map(|candidate| candidate.coordinates);
    }

    Some(representative)
}

/// Union-find root lookup with path compression.
fn find(parent: &mut [usize], index: usize) -> usize {
    let mut root = index;
    while let Some(&next) = parent.get(root) {
        if next == root {
            break;
        }
        root = next;
    }

    let mut current = index;
    while let Some(slot) = parent.get_mut(current) {
        if *slot == root {
            break;
        }
        current = std::mem::replace(slot, root);
    }

    root
}

fn union(parent: &mut [usize], left: usize, right: usize) {
    let left = find(parent, left);
    let right = find(parent, right);
    if let Some(slot) = parent.get_mut(left.max(right)) {
        *slot = left.min(right);
    }
}

/// Groups listings that share a numeric ID, or an image key across different
/// sources (same-source image reuse is ignored so stock photos don't merge
/// distinct ads). Matching is transitive. Groups are ordered by their first
/// listing, and members keep their input order.
pub(crate) fn group_listings(listings: Vec<Listing>) -> Vec<Vec<Listing>> {
    let mut parent: Vec<usize> = (0..listings.len()).collect();
    let mut first_by_id: HashMap<&str, usize> = HashMap::new();
    let mut first_by_key: HashMap<(&str, ListingSource), usize> = HashMap::new();

    for (index, listing) in listings.iter().enumerate() {
        match first_by_id.get(listing.id.as_str()) {
            Some(&other) => union(&mut parent, index, other),
            None => {
                first_by_id.insert(&listing.id, index);
            }
        }

        for key in &listing.image_keys {
            for source in ListingSource::ALL {
                if source != listing.source
                    && let Some(&other) = first_by_key.get(&(key.as_str(), source))
                {
                    union(&mut parent, index, other);
                }
            }
            first_by_key
                .entry((key.as_str(), listing.source))
                .or_insert(index);
        }
    }

    let roots: Vec<usize> = (0..listings.len())
        .map(|index| find(&mut parent, index))
        .collect();
    let mut group_by_root: HashMap<usize, usize> = HashMap::new();
    let mut groups: Vec<Vec<Listing>> = Vec::new();

    for (listing, root) in listings.into_iter().zip(roots) {
        let position = *group_by_root.entry(root).or_insert_with(|| {
            groups.push(Vec::new());
            groups.len() - 1
        });
        if let Some(group) = groups.get_mut(position) {
            group.push(listing);
        }
    }

    groups
}

#[cfg(test)]
mod tests {
    use anyhow::{Context as _, Result};

    use super::{group_listings, representative};
    use crate::listing::{Listing, ListingSource};
    use crate::test_support;
    use crate::{daft_ie, rent_ie};
    fn listing(id: &str, image_keys: &[&str], source: ListingSource) -> Listing {
        Listing {
            image_keys: image_keys.iter().map(ToString::to_string).collect(),
            ..test_support::listing(id, source)
        }
    }

    #[test]
    fn groups_by_shared_id_or_image_key() {
        let groups = group_listings(vec![
            listing("1", &["a"], ListingSource::Rent),
            listing("2", &["b"], ListingSource::Rent),
            listing("3", &["a"], ListingSource::Daft),
            listing("2", &["c"], ListingSource::Daft),
        ]);

        assert_eq!(groups.len(), 2);
        assert_eq!(groups.first().map(Vec::len), Some(2));
        assert_eq!(groups.get(1).map(Vec::len), Some(2));
    }

    #[test]
    fn prefers_the_daft_listing_as_representative() {
        let group = vec![
            listing("1", &["a"], ListingSource::Rent),
            listing("2", &["a"], ListingSource::Daft),
            listing("3", &["a"], ListingSource::Property),
        ];

        let representative = representative(&group);

        assert_eq!(
            representative.as_ref().map(|listing| listing.source),
            Some(ListingSource::Daft)
        );
        assert_eq!(
            representative.as_ref().map(|listing| listing.id.as_str()),
            Some("2")
        );
    }

    #[test]
    fn keeps_unmatched_listings_separate() {
        let groups = group_listings(vec![
            listing("1", &["a"], ListingSource::Rent),
            listing("2", &["b"], ListingSource::Daft),
            listing("3", &[], ListingSource::Rent),
        ]);

        assert_eq!(groups.len(), 3);
    }

    #[test]
    fn does_not_group_same_source_image_reuse() {
        let groups = group_listings(vec![
            listing("1", &["a"], ListingSource::Rent),
            listing("2", &["a"], ListingSource::Rent),
        ]);

        assert_eq!(groups.len(), 2);
    }

    #[test]
    fn groups_same_source_id_repeats_and_keeps_input_order() {
        let groups = group_listings(vec![
            listing("1", &[], ListingSource::Rent),
            listing("2", &["x"], ListingSource::Daft),
            listing("1", &[], ListingSource::Rent),
            listing("3", &["x"], ListingSource::Property),
        ]);

        let ids: Vec<Vec<&str>> = groups
            .iter()
            .map(|group| group.iter().map(|listing| listing.id.as_str()).collect())
            .collect();
        assert_eq!(ids, vec![vec!["1", "1"], vec!["2", "3"]]);
    }

    #[test]
    fn groups_transitively() {
        let groups = group_listings(vec![
            listing("1", &["a"], ListingSource::Rent),
            listing("2", &["a", "b"], ListingSource::Daft),
            listing("3", &["b"], ListingSource::Rent),
        ]);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups.first().map(Vec::len), Some(3));
    }

    #[test]
    #[ignore = "uses the local archives/ fixtures"]
    fn groups_archived_cross_site_duplicates() -> Result<()> {
        let rent_json = std::fs::read_to_string(
            "archives/rentie/search_waterford_city_centre_rent_0-550_page_1.json",
        )
        .context("failed to read rent.ie fixture")?;
        let daft_json = std::fs::read_to_string(
            "archives/daftie/sharing_waterford_city_rent_0-550_page_1.json",
        )
        .context("failed to read daft.ie fixture")?;

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
