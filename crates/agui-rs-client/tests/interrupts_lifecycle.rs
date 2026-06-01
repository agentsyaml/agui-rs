//! Ported from TypeScript `agent/__tests__/interrupts-lifecycle.test.ts`.
//!
//! Covers `AbstractAgent.pendingInterrupts` + `runAgent` resume enforcement,
//! now implemented on the Rust stateful `AgentRunner`:
//! - a run that follows an interrupt outcome must address every open interrupt
//!   via `resume`, or the run is rejected;
//! - an expired pending interrupt rejects the run;
//! - a successful outcome clears the pending set;
//! - `clone_runner()` preserves the pending interrupt set (TS `clone()`).

use std::sync::Mutex;

use agui_rs_client::{AgUiError, Agent, AgentConfig, AgentRunner, RunAgentParameters};
use agui_rs_core::{
    factory, BaseEventFields, Event, Interrupt, Result, ResumeEntry, ResumeStatus, RunAgentInput,
    RunFinishedEvent, RunFinishedOutcome,
};
use async_trait::async_trait;
use futures::{stream, stream::BoxStream};

/// Agent whose emitted event sequence can be reconfigured between runs so a
/// single instance can first produce an interrupt outcome and then a success.
struct ScriptedAgent {
    scripts: Mutex<Vec<Vec<Event>>>,
}

impl ScriptedAgent {
    fn new(scripts: Vec<Vec<Event>>) -> Self {
        Self {
            scripts: Mutex::new(scripts),
        }
    }
}

#[async_trait]
impl Agent for ScriptedAgent {
    async fn run(&self, _input: RunAgentInput) -> Result<BoxStream<'static, Result<Event>>> {
        let events = {
            let mut scripts = self.scripts.lock().expect("scripts lock");
            if scripts.is_empty() {
                Vec::new()
            } else {
                scripts.remove(0)
            }
        };
        Ok(Box::pin(stream::iter(events.into_iter().map(Ok))))
    }
}

fn interrupt(id: &str, expires_at: Option<&str>) -> Interrupt {
    Interrupt {
        id: id.into(),
        reason: "tool_call".into(),
        message: None,
        tool_call_id: None,
        response_schema: None,
        expires_at: expires_at.map(str::to_string),
        metadata: None,
    }
}

fn run_finished_interrupt(interrupts: Vec<Interrupt>) -> Event {
    Event::RunFinished(RunFinishedEvent {
        thread_id: "thread-1".into(),
        run_id: "run-1".into(),
        result: None,
        outcome: Some(RunFinishedOutcome::Interrupt { interrupts }),
        base: BaseEventFields::default(),
    })
}

fn happy_run() -> Vec<Event> {
    vec![
        factory::run_started("thread-1", "run-1"),
        factory::run_finished("thread-1", "run-1"),
    ]
}

fn interrupting_run(interrupts: Vec<Interrupt>) -> Vec<Event> {
    vec![
        factory::run_started("thread-1", "run-1"),
        run_finished_interrupt(interrupts),
    ]
}

fn resolved(id: &str) -> ResumeEntry {
    ResumeEntry {
        interrupt_id: id.into(),
        status: ResumeStatus::Resolved,
        payload: None,
    }
}

#[tokio::test]
async fn allows_run_when_pending_interrupts_is_empty() {
    let agent = ScriptedAgent::new(vec![happy_run()]);
    let mut runner = AgentRunner::new(agent, AgentConfig::default());

    let result = runner.run_agent(RunAgentParameters::default()).await;
    assert!(result.is_ok());
    assert!(runner.pending_interrupts().is_empty());
}

#[tokio::test]
async fn populates_pending_interrupts_from_interrupt_outcome() {
    let agent = ScriptedAgent::new(vec![interrupting_run(vec![
        interrupt("int-1", None),
        interrupt("int-2", None),
    ])]);
    let mut runner = AgentRunner::new(agent, AgentConfig::default());

    runner
        .run_agent(RunAgentParameters::default())
        .await
        .expect("first run should succeed");

    let pending: Vec<&str> = runner
        .pending_interrupts()
        .iter()
        .map(|i| i.id.as_str())
        .collect();
    assert_eq!(pending, vec!["int-1", "int-2"]);
}

#[tokio::test]
async fn allows_run_when_resume_covers_every_pending_interrupt() {
    let agent = ScriptedAgent::new(vec![
        interrupting_run(vec![interrupt("int-1", None), interrupt("int-2", None)]),
        happy_run(),
    ]);
    let mut runner = AgentRunner::new(agent, AgentConfig::default());

    runner
        .run_agent(RunAgentParameters::default())
        .await
        .expect("first run should succeed");

    let result = runner
        .run_agent(RunAgentParameters {
            resume: vec![
                resolved("int-1"),
                ResumeEntry {
                    interrupt_id: "int-2".into(),
                    status: ResumeStatus::Cancelled,
                    payload: None,
                },
            ],
            ..RunAgentParameters::default()
        })
        .await;

    assert!(result.is_ok());
    // Success outcome clears the pending set.
    assert!(runner.pending_interrupts().is_empty());
}

#[tokio::test]
async fn rejects_run_when_resume_is_missing() {
    let agent = ScriptedAgent::new(vec![interrupting_run(vec![interrupt("int-1", None)])]);
    let mut runner = AgentRunner::new(agent, AgentConfig::default());

    runner
        .run_agent(RunAgentParameters::default())
        .await
        .expect("first run should succeed");

    let result = runner.run_agent(RunAgentParameters::default()).await;
    let error = match result {
        Ok(_) => panic!("missing resume should reject the run"),
        Err(error) => error,
    };
    match error {
        AgUiError::Validation(message) => {
            assert!(message.contains("pending interrupt"), "got: {message}");
            assert!(message.contains("int-1"), "got: {message}");
        }
        other => panic!("expected validation error, got {other:?}"),
    }
}

#[tokio::test]
async fn rejects_run_when_resume_does_not_cover_every_pending_interrupt() {
    let agent = ScriptedAgent::new(vec![interrupting_run(vec![
        interrupt("int-1", None),
        interrupt("int-2", None),
    ])]);
    let mut runner = AgentRunner::new(agent, AgentConfig::default());

    runner
        .run_agent(RunAgentParameters::default())
        .await
        .expect("first run should succeed");

    let result = runner
        .run_agent(RunAgentParameters {
            resume: vec![resolved("int-1")],
            ..RunAgentParameters::default()
        })
        .await;
    let error = match result {
        Ok(_) => panic!("partial resume should reject the run"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("int-2"), "got: {error}");
}

#[tokio::test]
async fn rejects_run_when_pending_interrupt_is_expired() {
    let agent = ScriptedAgent::new(vec![interrupting_run(vec![interrupt(
        "int-1",
        Some("2000-01-01T00:00:00Z"),
    )])]);
    let mut runner = AgentRunner::new(agent, AgentConfig::default())
        .with_now_fn(|| "2026-05-30T00:00:00Z".to_string());

    runner
        .run_agent(RunAgentParameters::default())
        .await
        .expect("first run should succeed");

    let result = runner
        .run_agent(RunAgentParameters {
            resume: vec![resolved("int-1")],
            ..RunAgentParameters::default()
        })
        .await;
    let error = match result {
        Ok(_) => panic!("expired interrupt should reject the run"),
        Err(error) => error,
    };
    assert!(
        error.to_string().to_lowercase().contains("expired"),
        "got: {error}"
    );
}

// IMPLEMENTED: TypeScript AbstractAgent.clone() pendingInterrupts preservation —
// the Rust `AgentRunner::clone_runner` deep-copies the pending interrupt set, so
// a clone taken mid-interrupt enforces resume coverage exactly like its source.
#[tokio::test]
async fn clone_runner_preserves_pending_interrupts() {
    let agent = ScriptedAgent::new(vec![interrupting_run(vec![
        interrupt("int-1", None),
        interrupt("int-2", None),
    ])]);
    let mut runner = AgentRunner::new(agent, AgentConfig::default());

    runner
        .run_agent(RunAgentParameters::default())
        .await
        .expect("first run should succeed");

    let cloned = runner.clone_runner();
    let original_ids: Vec<&str> = runner
        .pending_interrupts()
        .iter()
        .map(|interrupt| interrupt.id.as_str())
        .collect();
    let cloned_ids: Vec<&str> = cloned
        .pending_interrupts()
        .iter()
        .map(|interrupt| interrupt.id.as_str())
        .collect();
    assert_eq!(original_ids, vec!["int-1", "int-2"]);
    assert_eq!(cloned_ids, original_ids);
}
