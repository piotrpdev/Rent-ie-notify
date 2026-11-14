# Rent.ie Notify

Scans rent.ie and/or daft.ie for new listings and uses a Slack webhook to notify you. Listings found on both sites are grouped by shared numeric ID or media key and sent once.

Set `SLACK_WEBHOOK_URL` and at least one source:

- `RENT_IE_URL` — a rent.ie search-results URL.
- `DAFT_IE_REQUEST` — the JSON request body for `https://gateway.daft.ie/api/v2/ads/listings`.

Optional:

- `PLACES_OF_INTEREST` — JSON array of `{"name","latitude","longitude"}` places for distance lines.
- `OPENROUTESERVICE_API_KEY` — enables OpenRouteService driving time/distance (`driving-car`) with straight-line fallback.

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

`RENT_IE_URL` and `DAFT_IE_REQUEST` are picked up from the environment unless overridden with `--rent-url` / `--daft-request`. OpenRouteService is disabled during this check by default; pass `--with-routing` to forward `OPENROUTESERVICE_API_KEY` and exercise routing.

## License

Licensed under the MIT license.
