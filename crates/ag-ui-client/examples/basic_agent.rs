use ag_ui_client::{AgentConfig, AgentRunner, HttpAgent, HttpAgentConfig, RunAgentParameters};

#[tokio::main]
async fn main() -> ag_ui_core::Result<()> {
    let agent = HttpAgent::new(HttpAgentConfig {
        url: "http://localhost:8000/".to_string(),
        headers: Default::default(),
        agent: AgentConfig::default(),
        request_executor: None,
    });

    let mut runner = AgentRunner::new(agent, AgentConfig::default());
    let result = runner.run_agent(RunAgentParameters::default()).await?;
    println!(
        "Run finished: {} messages, state={:?}",
        result.new_messages.len(),
        result.new_state
    );
    Ok(())
}
