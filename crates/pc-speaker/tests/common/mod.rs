use super::Speaker;
use relocade_machine::{Clock, IoBus};
use relocade_pit::Pit;
use std::{cell::RefCell, rc::Rc};

/// Connect the real PIT and speaker through their separate port ranges.
pub fn hardware(clock: Clock) -> (IoBus, Rc<RefCell<Speaker>>) {
    let pit = Rc::new(RefCell::new(Pit::with_clock(
        clock,
        &relocade_pic::Pic::new(0x08, 0x70),
    )));
    let speaker = Rc::new(RefCell::new(Speaker::new(pit.clone())));
    let io = IoBus::default();
    io.connect(pit.clone(), |io| Pit::configure_io(io, &pit));
    io.connect(speaker.clone(), |io| Speaker::configure_io(io, &speaker));
    (io, speaker)
}
