#![cfg(not(target_arch = "wasm32"))]

use futures_channel::oneshot;
use relocade_dos::Dos;
use relocade_frontend::{
    keyboard::{ElementState, Key, KeyCode, KeyLocation, KeyboardEvent},
    mouse::{MouseButton, MouseEvent},
};
use relocade_machine::{Clock, ExecutionWorker, Hook};
use std::{
    cell::RefCell,
    future::{poll_fn, Future},
    pin::pin,
    sync::mpsc,
    task::Poll,
    time::Duration,
};

#[derive(Clone, Copy)]
struct FrozenClock;
impl Clock for FrozenClock {
    fn now(&self) -> Duration {
        Duration::ZERO
    }
    async fn wait_until(&self, _: Duration) {
        std::future::pending::<()>().await;
    }
}

fn controller_write(code: &mut Vec<u8>, port: u8, value: u8) {
    code.extend([
        0xe4, 0x64, 0xa8, 2, 0x75, 0xfa, // Wait until IBF clears.
        0xb0, value, 0xe6, port,
    ]);
}

#[test]
fn guest_configures_auxiliary_mouse_and_receives_irq12_from_host_input() {
    let (ready, host_inputs) = mpsc::channel();
    let (resume, resumed) = oneshot::channel();
    let worker = ExecutionWorker::spawn(FrozenClock, async move |runner| {
        let mut dos = Dos::new(runner).await.unwrap();
        let mut code = vec![0xfa]; // Configure while IF is clear.
        controller_write(&mut code, 0x64, 0x60);
        controller_write(&mut code, 0x60, 0x45); // Auxiliary enabled, IRQ12 disabled.
        controller_write(&mut code, 0x64, 0xd4);
        controller_write(&mut code, 0x60, 0xf0); // Remote mode needs no host timer.
        code.extend([
            0xe4, 0x64, 0xa8, 1, 0x74, 0xfa, // Wait for ACK in OBF.
            0xe4, 0x60, 0xa2, 0x10, 5, // Save ACK at 0510h.
        ]);
        controller_write(&mut code, 0x64, 0x60);
        controller_write(&mut code, 0x60, 0x47); // Enable auxiliary IRQs.
        code.extend([
            0xb0, 0xf8, 0xe6, 0x21, // Unmask IRQ0/IRQ1/master cascade.
            0xb0, 0xef, 0xe6, 0xa1, // Unmask slave IRQ12.
            0xeb, 0, // Dispatch the host rendezvous.
        ]);
        let rendezvous = 0x100 + code.len() as u32;
        code.extend([0xeb, 0]);
        let continuation = 0x100 + code.len() as u32;
        code.push(0xfb); // Resume with IRQ1/IRQ12 enabled.
        controller_write(&mut code, 0x64, 0xd4);
        controller_write(&mut code, 0x60, 0xeb); // Read Data: ACK plus a three-byte report.
        code.extend([
            0x83, 0x3e, 0, 5, 4, 0x72, 0xf9, // Wait for four IRQ12 bytes.
            0x31, 0xc0, 0xcd, 0x16, // BIOS key read through ordinary IRQ1.
            0xa3, 0x12, 5, // Save BIOS key at 0512h.
            0x89, 0x26, 0x14, 5, // Save SP after all interrupt frames return.
            0xb8, 0, 0x4c, 0xcd, 0x21, // Exit successfully.
        ]);
        dos.launch_com(&code, Default::default()).unwrap();
        let segment = dos.process().unwrap().psp_segment();
        let base = u32::from(segment) << 4;
        let memory = dos.cpu().memory();
        memory
            .view()
            .write_u32(0x74 * 4, (u32::from(segment) << 16) | 0x300)
            .unwrap();
        memory
            .view()
            .write(
                base + 0x300,
                &[
                    0x50, 0x53, 0x1e, 0x0e, 0x1f, // Save AX/BX/DS; DS=CS.
                    0x8b, 0x1e, 0, 5, // BX = received byte count.
                    0xe4, 0x64, 0x88, 0x87, 0x20, 5, // Log status at [0520h+BX].
                    0xe4, 0x60, 0x88, 0x87, 2, 5, // Log data at [0502h+BX].
                    0xff, 0x06, 0, 5, // Increment byte count.
                    0xb0, 0x20, 0xe6, 0xa0, 0xe6, 0x20, // EOI slave, then master.
                    0x1f, 0x5b, 0x58, 0xcf, // Restore; IRET.
                ],
            )
            .unwrap();
        let state = dos.cpu().cpu_state();
        let stack = state.sp();
        let mouse = dos.mouse_input();
        let keyboard = dos.keyboard_input();
        let ready = RefCell::new(Some(ready));
        let resumed = RefCell::new(Some(resumed));
        dos.cpu()
            .install([Hook::host(base + rendezvous, async move || {
                let ready = ready.borrow_mut().take().unwrap();
                let resumed = resumed.borrow_mut().take().unwrap();
                state.set_eip(continuation);
                ready.send((mouse.clone(), keyboard.clone())).unwrap();
                resumed.await.unwrap();
                // Drain the finite input burst before the guest requests its snapshot.
                // IF stays clear while this raw host hook is suspended.
                for _ in 0..4 {
                    relocade_machine::yield_now().await;
                }
            })])
            .await
            .unwrap();
        assert_eq!(dos.run(runner).await, Ok(0));
        assert_eq!(memory.view().read_u8(base + 0x510).unwrap(), 0xfa);
        assert_eq!(memory.view().read_u16(base + 0x500).unwrap(), 4);
        let mut packet = [0; 4];
        memory.view().read(base + 0x502, &mut packet).unwrap();
        assert_eq!(packet, [0xfa, 0x29, 7, 0xfd]);
        let mut statuses = [0; 4];
        memory.view().read(base + 0x520, &mut statuses).unwrap();
        assert!(statuses.into_iter().all(|status| status & 0x21 == 0x21));
        assert_eq!(memory.view().read_u16(base + 0x512).unwrap(), 0x1e61);
        assert_eq!(
            memory.view().read_u16(base + 0x514).unwrap(),
            stack,
            "IRQ frames returned before DOS exit"
        );
    });
    let (mouse, keyboard) = host_inputs.recv_timeout(Duration::from_secs(30)).unwrap();
    mouse
        .unbounded_send(MouseEvent::Button {
            button: MouseButton::Left,
            state: ElementState::Pressed,
        })
        .unwrap();
    mouse
        .unbounded_send(MouseEvent::Motion {
            delta_x: 7.0,
            delta_y: 3.0,
        })
        .unwrap();
    keyboard
        .unbounded_send(KeyboardEvent::Key {
            physical_key: KeyCode::KeyA.into(),
            logical_key: Key::Character("a".into()),
            location: KeyLocation::Standard,
            text: Some("a".into()),
            state: ElementState::Pressed,
            repeat: false,
        })
        .unwrap();
    resume.send(()).unwrap();
    pollster::block_on(async {
        let mut join = pin!(worker.join());
        let mut timeout = pin!(futures_timer::Delay::new(Duration::from_secs(30)));
        let result = poll_fn(|cx| {
            if let Poll::Ready(result) = join.as_mut().poll(cx) {
                return Poll::Ready(result);
            }
            assert!(
                timeout.as_mut().poll(cx).is_pending(),
                "mouse guest timed out"
            );
            Poll::Pending
        })
        .await;
        assert_eq!(result.unwrap(), Some(()));
    });
}
