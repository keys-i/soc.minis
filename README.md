# SOC Minis

SOC Minis is the standalone Eventy service for student societies

It keeps event preparation, editable risk documents and booking follow-up together while keeping mail authority explicit

## Run locally

```sh
cargo run -- serve
npm --prefix frontend install
npm --prefix frontend run dev
```

The Rust binary also owns both terminal workflows

```sh
cargo run -- event examples/movie-night.toml --risk --form --output-dir out/event
cargo run -- agent status
```

Copy `.env.example` to `.env` only for local development and never commit credentials

## Host behind the private edge

Build the release without exposing an application port

```sh
npm --prefix frontend ci
npm --prefix frontend run build
cargo build --locked --release
```

Install the binary at `/opt/soc-minis/current/soc`, the built frontend at `/opt/soc-minis/current/frontend`, and [the systemd unit](deploy/soc-minis.service) at `/etc/systemd/system/soc-minis.service`

Create the `soc-minis` system user and group, add only Caddy to that group, and keep cloudflared out of it
The unit creates `/run/soc-minis/app.sock` as a private `0660` Unix socket and keeps durable state in `/var/lib/soc-minis`

Set `SOC_ACCESS_ISSUER` and `SOC_ACCESS_AUDIENCE` in root-owned `/etc/soc-minis/service.env`
Hosted requests are accepted only when the Cloudflare Access JWT signature and claims verify
The same-host Unix socket ACL replaces the old transitional origin secret

Verify the service before adding it to a release gate

```sh
sudo systemctl daemon-reload
sudo systemctl enable --now soc-minis.service
sudo -u caddy curl --fail --unix-socket /run/soc-minis/app.sock http://localhost/health/ready
```

