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

