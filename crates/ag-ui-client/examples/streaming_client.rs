//! Streaming subscriber example: connects to any AG-UI server (e.g. the
//! `echo_agent` example shipped with `ag-ui-server`) and prints every event
//! it observes via `AgentSubscriber::on_event`. Mirrors the TS
//! `AgentSubscriber` demo from the upstream `ag-ui-protocol/ag-ui` reference
//! repository.
//!
//! Run (in another terminal start the server first):
//!     cargo run -p ag-ui-server --example echo_agent
//!     cargo run -p ag-ui-client --example streaming_client

use std::sync::Arc;

use ag_ui_client::{
    AgentConfig, AgentRunner, AgentSubscriber, HttpAgent, HttpAgentConfig, RunAgentParameters,
    RunContext,
};
use ag_ui_core::{Event, Message, UserMessageContent};
use async_trait::async_trait;

struct PrintSubscriber;

#[async_trait]
impl AgentSubscriber for PrintSubscriber {
    async fn on_event(&self, ctx: &RunContext, event: &Event) {
        println!(
            "[event] thread={} run={} type={:?}",
            ctx.thread_id,
            ctx.run_id,
            event.event_type()
        );
    }
}

#[tokio::main]
async fn main() -> ag_ui_core::Result<()> {
    let user_msg = Message::User(ag_ui_core::types::UserMessage {
        id: "u1".to_string(),
        content: UserMessageContent::Text("hello from streaming_client".to_string()),
        name: None,
        encrypted_value: None,
    });

    let agent_cfg = AgentConfig {
        thread_id: Some("t1".to_string()),
        initial_messages: vec![user_msg],
        ..Default::default()
    };

    let agent = HttpAgent::new(HttpAgentConfig {
        url: "http://127.0.0.1:8000/".to_string(),
        headers: Default::default(),
        agent: agent_cfg.clone(),
    });

    let mut runner = AgentRunner::new(agent, agent_cfg).with_subscriber(Arc::new(PrintSubscriber));

    let result = runner.run_agent(RunAgentParameters::default()).await?;

    println!(
        "\n[done] new_messages={} new_state={}",
        result.new_messages.len(),
        result.new_state
    );
    Ok(())
}
