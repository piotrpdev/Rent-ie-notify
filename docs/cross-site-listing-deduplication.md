# Cross-site listing deduplication

## Goal

Group or suppress listings that are the same property posted on both rent.ie and
daft.ie. The poster may copy details with slight differences, so matching should
use several signals rather than address alone.

## Initial observations

- The example rent.ie and daft.ie property URLs both use `id=6721137`, suggesting
  the two sites share a numeric listing ID space.
- If that is reliable, exact cross-site ID matching is the strongest and cheapest
  duplicate signal.
- The first image from the same example on both sites uses the identical
  underlying media key:
  `9/b/9bc722bf-1a8a-4f48-a196-a74b88c1da2d.jpg`.
  Only the watermark, domain, and signature differ; the bucket, crop/resize
  options, output format, and source image key are identical.
- The search/detail response shapes are also extremely similar, which suggests a
  shared platform/backend rather than two independent systems. This makes shared
  numeric IDs and shared media keys expected rather than accidental.
- Earlier archived rent.ie and daft.ie search responses did not overlap by ID
  because they came from different searches/filters. A later matched-filter
  sample found 17/17 shared IDs (see the verified sample below).
- Address-only matching is risky. The archives contain a likely cautionary
  example:
  - rent.ie: `Barrack Street, Waterford...`, `€200–€300/week`
  - daft.ie: `Barrack Street, Waterford...`, `€250/week`
  - Different IDs, prices, and seller names. This could be the same room, a
    different room in the same building, or a separate advert.

## Verified sample: Waterford sharing listings under €550

Archived API responses:

- `archives/rentie/search_waterford_city_centre_rent_0-550_page_1.json`
- `archives/daftie/sharing_waterford_city_rent_0-550_page_1.json`

Results:

- rent.ie: 17 listings
- daft.ie: 17 listings
- Shared numeric IDs: all 17
- Every pair had matching titles, prices, and seller names.
- Every pair had overlapping image key sets, ranging from 1 to 9 shared images.
- No fuzzy matching was needed for this sample; exact ID + image-key matching
  identifies every duplicate.
- This also shows why the narrower search matters: the earlier Barrack Street
  cautionary pair came from different/wider searches. In this sample, the
  Barrack Street ad (ID `6706767`) matches its daft.ie counterpart exactly.

## Verified sample: Waterford residential listings under €1800

Archived API responses:

- `archives/rentie/search_waterford_city_centre_houses_rent_0-1800_page_1.json`
- `archives/rentie/search_waterford_city_centre_houses_rent_0-1800_page_2.json`
- `archives/daftie/residential_waterford_city_rent_0-1800_page_1.json`
- `archives/daftie/residential_waterford_city_rent_0-1800_page_2.json`

Results:

- rent.ie: 26 listings
- daft.ie: 31 listings
- Shared numeric IDs: all 26 rent.ie listings exist in the daft.ie set.
- Every shared-ID pair also had overlapping image keys.
- No image-key-only matches remained among the unmatched daft.ie listings.
- daft.ie had 5 extra listings not returned by the rent.ie search. Four are in
  Slieverue/Kilkenny and one is `3 The Mews, Rockshire Road, Ferrybank`, so they
  may simply be outside the narrower rent.ie area/filter rather than true
  duplicates.

## Matching signals, strongest first

### 1. Shared numeric ID

- Strip the current `daft-` prefix and compare the underlying numeric ID.
- Optionally sanity-check title/price similarity before grouping, to protect
  against an unlikely ID collision.
- This is low-risk if the ID spaces are shared, and requires no fuzzy matching.

### 2. Image fingerprints

- The media URL is a base64-encoded JSON descriptor. Strip the
  `?signature=...` query, base64-decode the path, and extract the `key` field.
- The verified example decodes to the same source key on both sites:
  `9/b/9bc722bf-1a8a-4f48-a196-a74b88c1da2d.jpg`.
- The descriptors differ only in the watermark overlay (`watermark-rent-logo-medium.png`
  vs `watermark-daft-logo-small2-v4.png`), domain, and signature.
- Compare sets of source image keys across all available images, ignoring order.
  A single key match plus another signal is strong; multiple matching keys are
  stronger.
- Be slightly cautious about agents reusing a stock/building photo across
  genuinely different rooms, so combine image matching with price/address/room
  details.
- If images are re-uploaded separately and keys differ, a perceptual image hash
  could be added later, but that requires downloading images and an image
  hashing crate.

### 3. Normalized address/title

- Lowercase, strip punctuation, collapse whitespace.
- Normalize common abbreviations:
  - `rd` / `road`
  - `st` / `street`
  - `co` / `county`
  - `apt` / `apartment`
- Use token-set similarity or Jaro-Winkler rather than exact equality to tolerate
  typos and small copy differences.
- Prefer structured address fields from property-detail endpoints when
  available.

### 4. Price

- Parse the numeric price.
- Normalize weekly prices to monthly equivalents.
- Allow a small tolerance for rounding/differences.
- Handle ranges such as `€450–€650` by overlap or midpoint.
- Price alone must not cause a match.

### 5. Bedrooms, bathrooms, property type

- Normalize values such as `2 Bed`, `2 Bedroom`, and `2-bed`.
- Use these as blocking/filtering attributes before expensive fuzzy comparisons.
- Room shares in the same building may differ by room, so beds alone is not
  enough to merge.

### 6. Seller identity

- Compare normalized seller names.
- Use seller IDs, phone numbers, or agent licence numbers from property-detail
  endpoints when available.
- Private sellers may be listed only as `Private User`, so absence of a seller
  match should not rule out a duplicate.

### 7. Description and availability details

- Property-detail endpoints include descriptions, property overview, facilities,
  BER, and availability information.
- Fetch these only for candidate pairs, not every listing.
- Compare descriptions with token overlap/cosine similarity to tolerate rewording
  and spelling mistakes.
- Availability-from/for and facilities are useful corroborating signals.

## Suggested matching strategy

1. Build normalized records for every listing from both sources.
2. Union records with the same numeric ID across sources.
3. For remaining records, generate candidates that are plausibly comparable:
   - similar bedroom/bathroom counts
   - compatible property type
   - price within tolerance
   - address/title similarity above a low threshold, or overlapping image keys
4. Score candidates:
   - shared ID: highest confidence
   - overlapping image keys: very high confidence
   - high title similarity + matching price + matching seller/details: high
   - address-only or price-only: insufficient
5. Group only above a conservative threshold.
6. Keep uncertain candidates visible in a diagnostic report instead of silently
   merging them.

## Grouping versus suppression

The implemented version groups cross-source duplicates and sends only the first
source's representative, suppressing the rest. This keeps Slack notifications
quiet and avoids exposing two nearly identical messages.

If false positives ever become a problem, the safer fallback is to show one block
with both links instead:

```text
*<rent.ie link|Address>*
Also on daft.ie: <daft.ie link>
...
```

That was considered as an initial rollout option, but the verified ID/image-key
matches were strong enough to go straight to suppression.

## State and dedupe behavior

Implemented behavior:

- rent.ie and daft.ie share numeric IDs. `sent_ids.txt` stores numeric IDs;
  legacy `<slug>/<id>` and `daft-<id>` entries are normalized when loaded.
- Listings are grouped by shared numeric ID or intersecting decoded image keys.
  Image-key matching only applies across different sources, so reusing a photo
  within one site does not merge distinct ads.
- The first source configured is the Slack representative for a group.
- A group is skipped if any member ID was already sent. When a new image-key
  group has different IDs, all of them are written to `sent_ids.txt`.
- Exact-ID duplicates across runs are handled. A cross-run image-key-only
  duplicate with a different ID can still come through; that is the intentional
  conservative tradeoff.

A richer state file such as `sent_listings.json` could later store image keys and
group fingerprints to catch cross-run image-key-only duplicates, but it would
also need to be persisted by the workflow/systemd deployment.

## Implementation status

Implemented in `src/dedupe.rs`:

1. Group exact shared-ID matches.
2. Decode media descriptors and match image keys across sources.
3. Keep the first source's listing as the representative and suppress the rest
   from Slack.
4. Save every ID in each new group so future exact-ID duplicates are skipped.

Possible later work:

- Add normalized detail/address/price/seller matching for lower-confidence
  candidates.
- Store image-key/group fingerprints for cross-run fuzzy duplicates.
- Add a diagnostic mode that prints why candidates were grouped.

## Testing ideas

- Exact same ID across sources.
- Same address with a typo or abbreviation difference.
- Weekly price versus equivalent monthly price.
- Price range versus a single price.
- Same building/address but different room, photos, seller, or price: must not
  merge.
- Same photos with different IDs: should match.
- Same seller/agent with several units at one address: should not merge solely on
  seller/address.
- Add the Barrack Street pair as an uncertain/negative case until more evidence
  is available.

## Open questions

- Should duplicates be grouped with both links initially, or suppressed outright?
- Is it acceptable to fetch property-detail endpoints for candidate pairs?
- Should image downloads/perceptual hashing be considered, or is media-key
  matching sufficient?
- How should `sent_ids.txt` be migrated to canonical IDs?
- How much state can be added to the workflow/systemd deployment?
