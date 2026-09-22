# syntax=docker/dockerfile:1
# Praxis-VPS-Image: Binary (Discord-Voice via songbird) + Node für das
# gebündelte POML-CLI. Runtime-Assets (templates, workflows, skills, …)
# bettet die Binary per include_bytes! selbst ein (assets.rs).
#
# Build: bash deploy/build.sh          (Standard: vayayo/praxis:latest)
# Benötigt POML-Checkout als Build-Kontext ../poml (s. deploy/build.sh).

# ---------------------------------------------------------------- Build
FROM rust:1-bookworm AS praxis-build
# audiopus/libopus (songbird) braucht cmake/pkg-config; libssl-dev als
# Rückfall für künftige Nicht-rustls-Abhängigkeiten.
# TARGETARCH (BuildKit/buildx): Multi-Arch-Builds dürfen target/ und die
# Cargo-Registry NICHT ungeschützt teilen (Race beim Entpacken —
# aws-lc-sys-Crash 20.09.): target pro Arch ins eigene Unterverzeichnis,
# Registry mit sharing=locked.
ARG TARGETARCH
RUN apt-get update && apt-get install -y --no-install-recommends \
        cmake pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
# Von assets.rs eingebettete Verzeichnisse (include_bytes!).
COPY contexts/ contexts/
COPY templates/ templates/
COPY workflows/ workflows/
COPY integrations/ integrations/
COPY skills/ skills/
COPY static/ static/
COPY plugins/ plugins/
COPY migrations/ migrations/
# docs/ und examples/ sind RUNTIME-Assets (assets.rs bettet z. B.
# docs/SKILLS.md als onboard-/repair-Dokumentation ein).
COPY docs/ docs/
COPY examples/ examples/
RUN --mount=type=cache,target=/build/target \
    --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    CARGO_TARGET_DIR=/build/target/${TARGETARCH:-host} \
    cargo build --locked --release --features songbird --bin praxis \
    && cp /build/target/${TARGETARCH:-host}/release/praxis /praxis

# ------------------------------------------------------------- POML-CLI
# POML-CLI ist ein Webpack-Bundle; die externals (sharp, pdfjs-dist …)
# kopiert das CopyPlugin (per .vscodeignore-Whitelist) nach
# python/poml/node_modules — das Ergebnis ist self-contained und braucht
# zur Laufzeit nur den Node-Interpreter.
FROM node:22-bookworm AS poml-build
WORKDIR /poml
COPY --from=poml package.json package-lock.json tsconfig.json webpack.config.cli.js .vscodeignore ./
COPY --from=poml packages/ packages/
RUN --mount=type=cache,target=/root/.npm \
    npm ci \
    && npm run build-cli \
    && node python/poml/js/cli.js --help >/dev/null \
    && node python/poml/js/cli.js -i '<poml><aimessage>ok</aimessage></poml>' -o /dev/stdout >/dev/null \
    && node -e "require('/poml/python/poml/node_modules/sharp')"

# --------------------------------------------------------------- Runtime
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 1000 --create-home --shell /usr/sbin/nologin praxis
# Node-Interpreter aus dem offiziellen Image (kein apt-node, kein npm nötig).
COPY --from=poml-build /usr/local/bin/node /usr/local/bin/node
COPY --from=praxis-build /praxis /opt/praxis/bin/praxis
COPY --from=poml-build /poml/python/poml /opt/praxis/poml/python/poml
WORKDIR /opt/praxis
ENV POML_CLI=/opt/praxis/poml/python/poml/js/cli.js \
    HOME=/opt/praxis
# State-Verzeichnisse vorab mit richtiger Ownership: Docker übernimmt
# Rechte beim ersten Verwenden eines benannten Volumes aus dem Image.
RUN mkdir -p /opt/praxis/data /opt/praxis/contexts && chown -R praxis:praxis /opt/praxis
COPY deploy/docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh
RUN chmod 0755 /usr/local/bin/docker-entrypoint.sh
# Kein USER-Directive: Der Entrypoint startet als root (Key-Zustellung auf
# tmpfs + chown der State-Binds) und droppt per setpriv auf uid 1000.
ENTRYPOINT ["/usr/local/bin/docker-entrypoint.sh"]
# Kein EXPOSE: In der VPS-Compose teilt sich Praxis den Netzwerk-Namespace
# mit NetBird — Erreichbarkeit regeln NetBird-ACLs, nicht Port-Publishes.
VOLUME ["/opt/praxis/data", "/opt/praxis/contexts"]
HEALTHCHECK --interval=60s --timeout=5s --start-period=30s \
    CMD curl -fs http://127.0.0.1:${GATEWAY_PORT:-3537}/health >/dev/null || exit 1
CMD ["run"]
