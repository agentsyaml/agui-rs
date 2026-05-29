# agui-rs

Facade crate for the [AG-UI Rust SDK](https://github.com/agentsyaml/agui-rs).

Re-exports all sub-crates behind Cargo features so you only need one dependency.

## Features

| Feature   | Enables                     | Description                          |
|-----------|-----------------------------|--------------------------------------|
| `core`    | `agui-rs-core` (default)    | Protocol types, events, error types  |
| `encoder` | `agui-rs-encoder` + `core`  | SSE / protobuf encoding              |
| `client`  | `agui-rs-client` + `core`   | HTTP agent, runner, subscriber       |
| `server`  | `agui-rs-server` + `core` + `encoder` | axum server integration    |
| `full`    | all of the above            | Convenience shortcut                 |

## Quick start

```rust
use agui_rs::client::{HttpAgent, AgentRunner};
use agui_rs::prelude::*;
```

## License

Apache-2.0
