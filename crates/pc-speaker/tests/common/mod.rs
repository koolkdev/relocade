use super::{Speaker, PORT_RANGE};
use relocade_machine::{Clock, DeviceConnection, IoBus};
use relocade_pit::Pit;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

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

/// Connect the real PIT and speaker through their separate port ranges.
pub fn hardware(clock: Rc<ManualClock>) -> (IoBus, Rc<RefCell<Speaker<ManualClock>>>) {
    let pit = Rc::new(RefCell::new(Pit::with_clock(
        clock,
        &relocade_pic::Pic::new(0x08),
    )));
    let speaker = Rc::new(RefCell::new(Speaker::new(pit.clone())));
    let io = IoBus::new([
        DeviceConnection {
            device: pit,
            mmio: vec![],
            ports: vec![relocade_pit::PORT_RANGE],
        },
        DeviceConnection {
            device: speaker.clone(),
            mmio: vec![],
            ports: vec![PORT_RANGE],
        },
    ]);
    (io, speaker)
}
