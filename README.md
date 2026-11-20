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
- `DRY_RUN=1` — print the Slack payloads instead of posting them and leave `sent_ids.txt` untouched (`SLACK_WEBHOOK_URL` not required).
- `SEED_SENT_IDS=1` — mark every listing currently found as already sent without posting anything. Run once with this before enabling the timer (or after losing `sent_ids.txt`) so Slack only receives listings that appear afterwards.

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

curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain 1.100.0
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

Configuration and secrets live in `/etc/rent-ie-notify.env` (loaded through
`EnvironmentFile=`), not in the unit file, because unit files are
world-readable. Create it from the example and restrict it to root:

```sh
sudo install -m 600 -o root -g root rent-ie-notify.env.example /etc/rent-ie-notify.env
sudoedit /etc/rent-ie-notify.env
```

Set `SLACK_WEBHOOK_URL` and at least one of `RENT_IE_URL`, `DAFT_IE_REQUEST`,
or `PROPERTY_IE_URL`; optionally `MAX_BEDS`, `PLACES_OF_INTEREST`,
`OPENROUTESERVICE_API_KEY`, `TEXT_REPORT_DIR`, `TEXT_REPORT_INCLUDE_ALL`. The
example contains `xxxx` placeholders; replace or comment out every active line.

If you previously kept `Environment=` lines in the unit, move them into this
file and remove them from `/etc/systemd/system/check_rent_ie.service`.

To avoid posting every existing listing on the first run, seed the state once
after the reload below (this runs the service normally, but with
`SEED_SENT_IDS=1`, which records the current listings without posting):

```sh
sudo systemctl set-environment SEED_SENT_IDS=1
sudo systemctl start check_rent_ie.service
sudo systemctl unset-environment SEED_SENT_IDS
```

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
of each hour and catches up after a missed run. The service is `Type=oneshot`
and is not restarted on failure (for example a Cloudflare 403); the next hourly
run retries. Failed runs show up as `failed` in `systemctl status`.

## License

Licensed under the MIT license.
