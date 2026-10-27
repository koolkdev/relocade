use crate::TextScreen;
use arc_swap::ArcSwap;
use std::sync::Arc;

/// Read-only access to the latest completed VGA text screen.
/// Clones can cross workers and outlive the device. Each read retains one coherent
/// screen; publishing a replacement neither changes it nor queues older frames.
#[derive(Clone, Default)]
pub struct Display {
    screen: Arc<ArcSwap<TextScreen>>,
}

impl Display {
    pub fn screen(&self) -> Arc<TextScreen> {
        self.screen.load_full()
    }

    pub(super) fn publish(&self, screen: TextScreen) {
        self.screen.store(Arc::new(screen));
    }
}
