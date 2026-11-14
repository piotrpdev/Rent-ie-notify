# Rent.ie Notify

Scans rent.ie and/or daft.ie for new listings and uses a Slack webhook to notify you.

Set `SLACK_WEBHOOK_URL` and at least one source:

- `RENT_IE_URL` — a rent.ie search-results URL.
- `DAFT_IE_REQUEST` — the JSON request body for `https://gateway.daft.ie/api/v2/ads/listings`.

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

`RENT_IE_URL` and `DAFT_IE_REQUEST` are picked up from the environment unless overridden with `--rent-url` / `--daft-request`.

## License

Licensed under the MIT license.
