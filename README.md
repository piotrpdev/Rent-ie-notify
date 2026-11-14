# Rent.ie Notify

Scans rent.ie, daft.ie, and/or property.ie for new listings and uses a Slack webhook to notify you. Listings found on multiple sites are grouped by shared numeric ID or media key and sent once.

Set `SLACK_WEBHOOK_URL` and at least one source:

- `RENT_IE_URL` — a rent.ie search-results URL.
- `DAFT_IE_REQUEST` — the JSON request body for `https://gateway.daft.ie/api/v2/ads/listings`.
- `PROPERTY_IE_URL` — a property.ie search-results URL.

Optional:

- `PLACES_OF_INTEREST` — JSON array of `{"name","latitude","longitude"}` places for distance lines.
- `OPENROUTESERVICE_API_KEY` — enables OpenRouteService driving time/distance (`driving-car`) with straight-line fallback.
- `TEXT_REPORT_DIR` — optional directory for timestamped grouped text reports of new listings. Set `TEXT_REPORT_INCLUDE_ALL=1` to include every unique listing in the current search instead.

## Testing

Run all checks:

```sh
scripts/check.sh
```

Run the binary end-to-end against a local Slack sink (no real webhook, temporary `sent_ids.txt`):

```sh
cargo build --locked
cargo slack-sink-e2e --daft-request-file path/to/daft-request.json
```

`RENT_IE_URL`, `DAFT_IE_REQUEST`, and `PROPERTY_IE_URL` are picked up from the environment unless overridden with `--rent-url` / `--daft-request` / `--property-url`. OpenRouteService is disabled during this check by default; pass `--with-routing` to forward `OPENROUTESERVICE_API_KEY` and exercise routing.

## License

Licensed under the MIT license.
