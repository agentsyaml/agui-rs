use ag_ui_core::{Event, Result, RunAgentInput};
use futures::stream::BoxStream;

/// Run-scoped metadata extracted from [`RunAgentInput`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunContext {
    pub thread_id: String,
    pub run_id: String,
    pub parent_run_id: Option<String>,
}

impl RunContext {
    pub fn new(
        thread_id: impl Into<String>,
        run_id: impl Into<String>,
        parent_run_id: Option<String>,
    ) -> Self {
        Self {
            thread_id: thread_id.into(),
            run_id: run_id.into(),
            parent_run_id,
        }
    }
}

impl From<&RunAgentInput> for RunContext {
    fn from(input: &RunAgentInput) -> Self {
        Self::new(
            input.thread_id.clone(),
            input.run_id.clone(),
            input.parent_run_id.clone(),
        )
    }
}

#[async_trait::async_trait]
pub trait RunHandler: Send + Sync + 'static {
    /// Process the input and return a stream of AG-UI events.
    async fn handle(&self, input: RunAgentInput) -> Result<BoxStream<'static, Result<Event>>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_context_copies_ids_from_input() {
        let mut input = RunAgentInput::new("thread-1", "run-1");
        input.parent_run_id = Some("parent-1".into());

        let context = RunContext::from(&input);

        assert_eq!(context.thread_id, "thread-1");
        assert_eq!(context.run_id, "run-1");
        assert_eq!(context.parent_run_id.as_deref(), Some("parent-1"));
    }
}
