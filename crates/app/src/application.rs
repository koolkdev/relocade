use arc_swap::ArcSwapOption;
use futures_channel::mpsc;
use relocade_machine::TimerHandle;
use std::sync::Arc;

/// Launch data for one application execution owner. Mutable state and non-Send
/// futures are constructed by `run` on that worker. No graphics API is required.
pub trait Application: Send + 'static {
    type Input: Send + 'static;
    type Source: Send + Sync + 'static;
    type Output: Send + 'static;

    async fn run(self, context: Context<Self::Input, Self::Source>)
        -> Result<Self::Output, String>;
}

/// Worker-local input, an embedding-supplied timer handle, and source publication.
/// Sources remain available after execution completes; publishing schedules no UI work.
pub struct Context<I, S> {
    pub clock: TimerHandle,
    pub input: mpsc::UnboundedReceiver<I>,
    pub(crate) source: Arc<ArcSwapOption<S>>,
}
impl<I, S> Context<I, S> {
    pub fn publish(&self, source: S) {
        self.source.store(Some(Arc::new(source)));
    }
}
