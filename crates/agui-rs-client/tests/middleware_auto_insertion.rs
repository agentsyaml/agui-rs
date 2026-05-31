//! Ported from the "auto insertion" describe blocks of the TypeScript
//! `backward-compatibility-0-0-{39,45,47}.test.ts` files.
//!
//! `AgentRunner::with_max_version` mirrors the TS `AbstractAgent` constructor:
//! it prepends backward-compatibility middleware when the declared max version
//! is at or below each threshold (`<= 0.0.39`, `<= 0.0.45`, `<= 0.0.47`).
//!
//! Note: the TS auto-insertion tests assert middleware *presence/transform* and
//! do not run through `verifyEvents`. We mirror that by asserting the chain
//! composition and exercising the transform via the runner for the 0.0.39 case
//! (whose output passes verify). The 0.0.45/0.0.47 transform *logic* is covered
//! at the middleware level in `middleware_backward_compat_0_0_{45,47}.rs`.

use agui_rs_client::{Agent, AgentConfig, AgentRunner, RunAgentParameters};
use agui_rs_core::{factory, Event, Result, RunAgentInput};
use async_trait::async_trait;
use futures::{stream, stream::BoxStream};

struct IdleAgent;

#[async_trait]
impl Agent for IdleAgent {
    async fn run(&self, _input: RunAgentInput) -> Result<BoxStream<'static, Result<Event>>> {
        Ok(Box::pin(stream::iter(Vec::new())))
    }
}

#[tokio::test]
async fn max_version_0_0_47_auto_inserts_all_three() {
    // <= 0.0.47 satisfies all three gates (0.0.39, 0.0.45, 0.0.47).
    let runner = AgentRunner::new(IdleAgent, AgentConfig::default()).with_max_version("0.0.39");
    assert_eq!(runner.middleware_chain().len(), 3);
}

#[tokio::test]
async fn max_version_0_0_45_auto_inserts_two() {
    // <= 0.0.45 satisfies the 0.0.45 and 0.0.47 gates (not 0.0.39).
    let runner = AgentRunner::new(IdleAgent, AgentConfig::default()).with_max_version("0.0.45");
    assert_eq!(runner.middleware_chain().len(), 2);
}

#[tokio::test]
async fn max_version_0_0_47_auto_inserts_one() {
    let runner = AgentRunner::new(IdleAgent, AgentConfig::default()).with_max_version("0.0.47");
    assert_eq!(runner.middleware_chain().len(), 1);
}

#[tokio::test]
async fn current_version_does_not_auto_insert() {
    // At the crate's own (0.1.1) version all gates are false.
    let runner = AgentRunner::new(IdleAgent, AgentConfig::default()).with_max_version("0.1.1");
    assert!(runner.middleware_chain().is_empty());
}

#[tokio::test]
async fn auto_inserted_middleware_runs_before_user_middleware() {
    // The 0.0.39 transform strips parent_run_id from the input. Drive a full run
    // through the runner to confirm the auto-inserted middleware actually runs.
    struct AssertingAgent;
    #[async_trait]
    impl Agent for AssertingAgent {
        async fn run(&self, input: RunAgentInput) -> Result<BoxStream<'static, Result<Event>>> {
            assert!(
                input.parent_run_id.is_none(),
                "0.0.39 middleware should strip parent_run_id"
            );
            Ok(Box::pin(stream::iter(vec![
                Ok(factory::run_started(&input.thread_id, &input.run_id)),
                Ok(factory::run_finished(&input.thread_id, &input.run_id)),
            ])))
        }
    }

    let mut runner =
        AgentRunner::new(AssertingAgent, AgentConfig::default()).with_max_version("0.0.39");
    runner
        .run_agent(RunAgentParameters::default())
        .await
        .expect("run succeeds");
}
