use relocade_machine::Clock;
use std::{cell::Cell, rc::Rc, time::Duration};

#[derive(Clone, Default)]
pub struct ManualClock(pub Rc<Cell<Duration>>);

impl Clock for ManualClock {
    fn now(&self) -> Duration {
        self.0.get()
    }

    async fn wait_until(&self, _deadline: Duration) {
        std::future::pending().await
    }
}
