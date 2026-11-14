# Rent.ie Notify

Scans rent.ie and/or daft.ie for new listings and uses a Slack webhook to notify you.

Set `SLACK_WEBHOOK_URL` and at least one source:

- `RENT_IE_URL` — a rent.ie search-results URL.
- `DAFT_IE_REQUEST` — the JSON request body for `https://gateway.daft.ie/api/v2/ads/listings`.

## License

Licensed under the MIT license.
