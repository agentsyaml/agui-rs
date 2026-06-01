//! Ported from TypeScript `agent/__tests__/subscriber.test.ts` — the
//! subscriber-registry and temporary-subscriber cases that the Rust
//! multi-subscriber `AgentRunner` now supports.
//!
//! Not ported (architectural divergence, see docs/test-reconciliation.md):
//! `stopPropagation` / `AgentStateMutation` chaining, dev-mode frozen-input
//! checks, and `runSubscribersWithMutation`/`process undefined` — the Rust
//! subscriber model uses return-based hooks, not mutation objects.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use agui_rs_client::{
    Agent, AgentConfig, AgentRunner, AgentSubscriber, EventContext, RunAgentParameters, RunContext,
};
use agui_rs_core::{factory, Event, Result, RunAgentInput, RunStartedEvent};
use async_trait::async_trait;
use futures::{stream, stream::BoxStream};

struct FakeAgent {
    events: Vec<Event>,
}

#[async_trait]
impl Agent for FakeAgent {
    async fn run(&self, _input: RunAgentInput) -> Result<BoxStream<'static, Result<Event>>> {
        Ok(Box::pin(stream::iter(
            self.events.clone().into_iter().map(Ok),
        )))
    }
}

fn happy_agent() -> FakeAgent {
    FakeAgent {
        events: vec![
            factory::run_started("thread-1", "run-1"),
            factory::run_finished("thread-1", "run-1"),
        ],
    }
}

/// Counts `on_run_started_event` invocations.
#[derive(Default)]
struct CountingSubscriber {
    started: AtomicUsize,
}

#[async_trait]
impl AgentSubscriber for CountingSubscriber {
    async fn on_run_started_event(&self, _ctx: &EventContext<'_, RunStartedEvent>) -> Result<()> {
        self.started.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

/// Records run ids in shared order to verify registration-order invocation.
struct OrderingSubscriber {
    label: &'static str,
    log: Arc<Mutex<Vec<&'static str>>>,
}

#[async_trait]
impl AgentSubscriber for OrderingSubscriber {
    async fn on_run_started(&self, _ctx: &RunContext) {
        self.log.lock().expect("log").push(self.label);
    }
}

#[tokio::test]
async fn subscribe_increases_count_and_unsubscribe_removes_it() {
    let runner = AgentRunner::new(happy_agent(), AgentConfig::default());
    assert_eq!(runner.subscriber_count(), 0);

    let subscription = runner.subscribe(Arc::new(CountingSubscriber::default()));
    assert_eq!(runner.subscriber_count(), 1);

    subscription.unsubscribe();
    assert_eq!(runner.subscriber_count(), 0);
}

#[tokio::test]
async fn supports_multiple_subscribers() {
    let runner = AgentRunner::new(happy_agent(), AgentConfig::default());
    let _s1 = runner.subscribe(Arc::new(CountingSubscriber::default()));
    let _s2 = runner.subscribe(Arc::new(CountingSubscriber::default()));
    assert_eq!(runner.subscriber_count(), 2);
}

#[tokio::test]
async fn unsubscribe_only_removes_the_specific_subscriber() {
    let runner = AgentRunner::new(happy_agent(), AgentConfig::default());
    let s1 = runner.subscribe(Arc::new(CountingSubscriber::default()));
    let _s2 = runner.subscribe(Arc::new(CountingSubscriber::default()));
    assert_eq!(runner.subscriber_count(), 2);

    s1.unsubscribe();
    assert_eq!(runner.subscriber_count(), 1);
}

#[tokio::test]
async fn all_registered_subscribers_receive_events() {
    let mut runner = AgentRunner::new(happy_agent(), AgentConfig::default());
    let a = Arc::new(CountingSubscriber::default());
    let b = Arc::new(CountingSubscriber::default());
    runner.subscribe(a.clone());
    runner.subscribe(b.clone());

    runner
        .run_agent(RunAgentParameters::default())
        .await
        .expect("run succeeds");

    assert_eq!(a.started.load(Ordering::SeqCst), 1);
    assert_eq!(b.started.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn subscribers_are_invoked_in_registration_order() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let mut runner = AgentRunner::new(happy_agent(), AgentConfig::default());
    runner.subscribe(Arc::new(OrderingSubscriber {
        label: "first",
        log: log.clone(),
    }));
    runner.subscribe(Arc::new(OrderingSubscriber {
        label: "second",
        log: log.clone(),
    }));

    runner
        .run_agent(RunAgentParameters::default())
        .await
        .expect("run succeeds");

    assert_eq!(*log.lock().expect("log"), vec!["first", "second"]);
}

#[tokio::test]
async fn temporary_subscriber_participates_only_in_that_run() {
    let mut runner = AgentRunner::new(
        FakeAgent {
            events: vec![
                factory::run_started("thread-1", "run-1"),
                factory::run_finished("thread-1", "run-1"),
            ],
        },
        AgentConfig::default(),
    );
    let permanent = Arc::new(CountingSubscriber::default());
    runner.subscribe(permanent.clone());

    let temporary = Arc::new(CountingSubscriber::default());
    runner
        .run_agent_with(RunAgentParameters::default(), temporary.clone())
        .await
        .expect("run succeeds");

    // Both fired for this run.
    assert_eq!(permanent.started.load(Ordering::SeqCst), 1);
    assert_eq!(temporary.started.load(Ordering::SeqCst), 1);
    // Temporary subscriber was not retained.
    assert_eq!(runner.subscriber_count(), 1);
}
