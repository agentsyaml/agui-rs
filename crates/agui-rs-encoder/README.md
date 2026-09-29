# agui-rs-encoder

Wire-format encoder for AG-UI events.

Supports SSE (`text/event-stream`, the default) and protobuf
(`application/vnd.ag-ui.event+proto`) via the `agui-rs-proto` crate, with
`Accept` content negotiation mirroring upstream `media-type.ts`. Note that
`Accept: */*` and `Accept: application/*` select **protobuf**; send an explicit
`Accept: text/event-stream` for SSE.

## License

Apache-2.0
