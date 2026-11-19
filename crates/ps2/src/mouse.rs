//! Mouse firmware on the same execution owner as the shared controller.

mod motion;
mod protocol;
mod transmission;

use crate::{
    controller::{Controller, Port},
    optional_wait,
};
use futures_util::{select_biased, FutureExt, StreamExt};
use protocol::Protocol;
use relocade_frontend::mouse::{MouseEvent, MouseEvents};
use relocade_machine::{yield_now, Clock};

pub(super) async fn run(clock: Clock, events: MouseEvents, controller: &Controller) {
    let mut protocol = Protocol::new(clock.clone());
    let mut events = Some(events);
    loop {
        let wake = {
            let output = protocol
                .byte()
                .map(|byte| controller.send(Port::Auxiliary, byte));
            let input = events.as_mut().map(StreamExt::next);
            let sample = protocol
                .deadline()
                .map(|deadline| clock.sleep_until(deadline));
            select_biased! {
                byte = controller.receive_device(Port::Auxiliary).fuse() => Wake::Command(byte),
                () = optional_wait(output) => Wake::Sent,
                () = optional_wait(sample) => Wake::Sample,
                event = optional_wait(input) => Wake::Event(event),
            }
        };
        match wake {
            Wake::Command(byte) => protocol.command(byte),
            Wake::Sent => protocol.sent(),
            Wake::Event(event) => {
                if event.is_none() {
                    events = None;
                }
                protocol.event(event.unwrap_or(MouseEvent::FocusLost));
                // Bound the work per poll when the host has queued more motion.
                yield_now().await;
            }
            Wake::Sample => protocol.sample(),
        }
    }
}

enum Wake {
    Command(u8),
    Sent,
    Event(Option<MouseEvent>),
    Sample,
}
