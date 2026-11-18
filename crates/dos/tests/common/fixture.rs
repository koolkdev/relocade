use super::{runner, VideoInspection};
use relocade_clock_test_support::ManualClock;
use relocade_dos::Dos;
use relocade_machine::Device;
use std::{
    future::{poll_fn, Future},
    ops::{Deref, DerefMut},
    pin::pin,
    task::{Context, Waker},
};

/// Service tests step the CPU directly and control time when a device needs it.
pub struct DosFixture {
    dos: Dos,
    clock: ManualClock,
}

pub async fn dos() -> DosFixture {
    let clock = ManualClock::new();
    let (runner, _host, machine) = runner(clock.clock()).await;
    let mut dos = runner
        .run(std::future::pending::<()>(), async |runner| {
            Dos::new(runner).await.unwrap()
        })
        .await
        .unwrap();
    machine.shutdown().await.unwrap();
    dos.cpu().interrupt_handle().take();
    DosFixture { dos, clock }
}

impl Deref for DosFixture {
    type Target = Dos;

    fn deref(&self) -> &Dos {
        &self.dos
    }
}

impl DerefMut for DosFixture {
    fn deref_mut(&mut self) -> &mut Dos {
        &mut self.dos
    }
}

impl DosFixture {
    /// A BIOS call can suspend the test program until vertical retrace.
    pub async fn run_until(&mut self, eip: u32) -> relocade_x86::ExecutionExit {
        let mut run = pin!(super::run_until(self.dos.cpu(), eip));
        poll_fn(|cx| {
            let result = run.as_mut().poll(cx);
            if result.is_pending() {
                if let Some(deadline) = self.clock.next_deadline() {
                    self.clock.advance_to(deadline);
                }
            }
            result
        })
        .await
    }
}

/// Flush staged output, then allow one fresh retrace to sample the final writes.
pub async fn next_capture(dos: &DosFixture) -> relocade_vga::DisplaySnapshot {
    let frames = dos.frames();
    let mut video = pin!(relocade_vga::DispiAdapter::run(dos.vga()));
    let mut cx = Context::from_waker(Waker::noop());
    assert!(video.as_mut().poll(&mut cx).is_pending());
    dos.clock
        .advance_to(dos.clock.next_deadline().expect("VGA retrace"));
    assert!(video.as_mut().poll(&mut cx).is_pending());
    frames.recv().await.unwrap()
}
