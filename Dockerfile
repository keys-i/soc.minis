FROM rust:1.97-bookworm AS backend
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY assets ./assets
RUN cargo build --locked --release

FROM node:26-bookworm-slim AS frontend
WORKDIR /src/frontend
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci
COPY frontend ./
RUN npm run build

FROM debian:bookworm-slim
RUN useradd --system --home /var/lib/soc --create-home soc
COPY --from=backend /src/target/release/soc /usr/local/bin/soc
COPY --from=frontend /src/frontend/dist /opt/soc/frontend
COPY assets /opt/soc/assets
USER soc
ENV SOC_BIND=0.0.0.0:8080 SOC_BASE=/soc SOC_DATA=/var/lib/soc SOC_FRONTEND=/opt/soc/frontend SOC_ASSETS=/opt/soc/assets
EXPOSE 8080
ENTRYPOINT ["soc", "serve"]

