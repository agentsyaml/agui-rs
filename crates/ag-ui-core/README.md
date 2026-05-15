# ag-ui-core

Core data types and event definitions for the [AG-UI protocol](https://docs.ag-ui.com).

This crate is transport-agnostic and runtime-agnostic. It exposes:

- All protocol data types (`Message`, `Tool`, `Context`, `RunAgentInput`, `Interrupt`, ...)
- All 35 protocol events as a discriminated `Event` enum
- Event factory constructors

Wire format: JSON with `camelCase` field names; event variants use the
`SCREAMING_SNAKE_CASE` `type` discriminator from the spec.
