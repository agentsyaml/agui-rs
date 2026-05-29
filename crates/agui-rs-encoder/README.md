# agui-rs-encoder

Wire-format encoder for AG-UI events.

Currently supports SSE (`text/event-stream`). Protobuf (`application/vnd.ag-ui.event+proto`)
surface is stubbed — returns `AgUiError::Unsupported` until a proto crate lands.

## License

Apache-2.0
