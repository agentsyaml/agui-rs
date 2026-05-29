# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-05-29

### Added
- Initial public release on crates.io.
- `agui-rs-core`: protocol types, 33 event variants, factory helpers.
- `agui-rs-encoder`: SSE encoder + protobuf media-type negotiation surface.
- `agui-rs-client`: `HttpAgent`, `AgentRunner`, subscriber hooks, middleware chain.
- `agui-rs-server`: `axum` route builder, `RunHandler`, channel `EventEmitter`.
- `agui-rs`: facade crate with `core` / `encoder` / `client` / `server` / `full` features.

[0.1.0]: https://github.com/agentsyaml/agui-rs/releases/tag/v0.1.0
