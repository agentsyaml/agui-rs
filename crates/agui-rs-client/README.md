# agui-rs-client

Client SDK for the AG-UI protocol.

Provides `HttpAgent`, `AgentRunner`, middleware, chunk expansion, event verification,
and subscriber hooks for consuming AG-UI event streams over SSE.

```rust
use agui_rs_client::{AgentConfig, AgentRunner, HttpAgent, HttpAgentConfig, RunAgentParameters};

#[tokio::main]
async fn main() -> agui_rs_core::Result<()> {
    let agent = HttpAgent::new(HttpAgentConfig {
        url: "http://localhost:8000/".to_string(),
        headers: Default::default(),
        agent: AgentConfig::default(),
        request_executor: None,
    });
    let mut runner = AgentRunner::new(agent, AgentConfig::default());
    let result = runner.run_agent(RunAgentParameters::default()).await?;
    println!("{} messages received", result.new_messages.len());
    Ok(())
}
```

## License

Apache-2.0
