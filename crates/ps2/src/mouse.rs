//! Mouse firmware on the same execution owner as the shared controller.

mod motion;
mod protocol;
mod transmission;

use crate::controller::{Controller, Port};
use futures_util::{pin_mut, select_biased, stream::LocalBoxStream, FutureExt, StreamExt};
use protocol::Protocol;
use relocade_frontend::mouse::MouseEvent;
use relocade_machine::Clock;
use std::future::pending;

pub(super) async fn run(
    clock: Clock,
    events: LocalBoxStream<'static, MouseEvent>,
    controller: &Controller,
) {
    let mut protocol = Protocol::new(clock.clone());
    let mut events = Some(events);
    loop {
        let wake = {
            let command = controller.receive_device(Port::Auxiliary).fuse();
            let output = async {
                if let Some(byte) = protocol.byte() {
                    controller.send(Port::Auxiliary, byte).await;
                } else {
                    pending::<()>().await;
                }
            }
            .fuse();
            let event = async {
                match events.as_mut() {
                    Some(events) => events.next().await,
                    None => pending().await,
                }
            }
            .fuse();
            let timer = async {
                if let Some(deadline) = protocol.deadline() {
                    clock.sleep_until(deadline).await;
                } else {
                    pending::<()>().await;
                }
            }
            .fuse();
            pin_mut!(command, output, event, timer);
            select_biased! {
                byte = command => Wake::Command(byte),
                () = output => Wake::Sent,
                () = timer => Wake::Sample,
                event = event => Wake::Event(event),
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
