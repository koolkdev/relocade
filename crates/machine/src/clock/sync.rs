use super::{ClockSource, Instant, Sleep};
use std::sync::Arc;

/// A captured host deadline and its timer source, transferable with output.
/// Clones retain the same deadline. Waiting registers on the receiving owner;
/// dropping that wait cancels it without consuming this captured synchronization.
/// Keep the host timer service running until its output consumers finish waiting.
/// The default value is immediately ready, for output without a host mapping.
#[derive(Clone, Default)]
pub struct HostSync(Option<Deadline>);

#[derive(Clone)]
struct Deadline {
    source: Arc<dyn ClockSource + Send + Sync>,
    at: Instant,
}

impl HostSync {
    /// Captures a deadline on a transferable host source. Guest sources map
    /// their timestamp first, then delegate to their configured host clock.
    pub fn new(source: impl ClockSource + Send + Sync + 'static, at: Instant) -> Self {
        Self(Some(Deadline {
            source: Arc::new(source),
            at,
        }))
    }

    /// The fixed timestamp in the retained host source's timebase, if mapped.
    pub fn deadline(&self) -> Option<Instant> {
        self.0.as_ref().map(|deadline| deadline.at)
    }

    /// Waits through the retained host timer source. Already elapsed deadlines
    /// and output without a host mapping complete immediately.
    pub fn sync(&self) -> Sleep {
        match &self.0 {
            Some(deadline) if deadline.at > deadline.source.now() => {
                deadline.source.sleep_until(deadline.at)
            }
            _ => Sleep::new(async {}),
        }
    }
}
