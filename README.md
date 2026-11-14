# Rent.ie Notify

Scans rent.ie, daft.ie, and/or property.ie for new listings and uses a Slack webhook to notify you. Listings found on multiple sites are grouped by shared numeric ID or media key and sent once.

Set `SLACK_WEBHOOK_URL` and at least one source:

- `RENT_IE_URL` — a rent.ie search-results URL.
- `DAFT_IE_REQUEST` — the JSON request body for `https://gateway.daft.ie/api/v2/ads/listings`.
- `PROPERTY_IE_URL` — a property.ie search-results URL.
- `MAX_BEDS` — optional client-side maximum bedroom count applied across all sources.

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

## Raspberry Pi / systemd setup

Build the release binary on the Pi:

```sh
sudo apt update
sudo apt install -y build-essential cmake pkg-config curl

curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain 1.92.0
source "$HOME/.cargo/env"

cd ~/Rent-ie-notify
cargo build --locked --release
```

The binary is written to `~/Rent-ie-notify/target/release/Rent-ie-notify`.

### Install or override the units

If the service and timer are not installed yet:

```sh
sudo cp check_rent_ie.service check_rent_ie.timer /etc/systemd/system/
```

If an older copy already exists under `/usr/lib/systemd/system`, create
higher-priority `/etc` copies while preserving the existing environment:

```sh
sudo systemctl edit --full check_rent_ie.service
sudo systemctl edit --full check_rent_ie.timer
```

Do not edit files under `/usr/lib`; systemd gives `/etc` precedence.

### Configure the service

Edit `/etc/systemd/system/check_rent_ie.service` and set:

- `RENT_IE_URL` — your rent.ie search URL.
- `SLACK_WEBHOOK_URL` — your Slack webhook.
- `DAFT_IE_REQUEST` — your JSON request body, or comment it out if unused.
- Optional: `PROPERTY_IE_URL`, `MAX_BEDS`, `PLACES_OF_INTEREST`,
  `OPENROUTESERVICE_API_KEY`, `TEXT_REPORT_DIR`, `TEXT_REPORT_INCLUDE_ALL`.

The checked-in service file contains `xxxx` placeholders. The active
`DAFT_IE_REQUEST` line must be replaced or commented out before starting the
service.

Many of these values are secrets; keep the real webhook and API keys only in
the `/etc` unit or an untracked environment file, never in the repository.

### Reload and test

```sh
sudo systemctl daemon-reload
systemctl show -p FragmentPath check_rent_ie.service
systemctl show -p FragmentPath check_rent_ie.timer
```

Both should point at `/etc/systemd/system/...`.

Run one test:

```sh
sudo systemctl start check_rent_ie.service
systemctl status check_rent_ie.service
journalctl -u check_rent_ie.service -n 100 --no-pager
```

Enable the hourly timer:

```sh
sudo systemctl enable --now check_rent_ie.timer
sudo systemctl restart check_rent_ie.timer
systemctl status check_rent_ie.timer
systemctl list-timers --all | grep check_rent_ie
```

The timer uses `OnCalendar=hourly` and `Persistent=true`, so it runs at the top
of each hour and catches up after a missed run.

## License

Licensed under the MIT license.
