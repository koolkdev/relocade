//! AppHost input crosses the execution worker, PS/2 and BIOS into a DOS program.
use super::*;
use relocade_dos::Dos;
use relocade_machine::{real_mode_interrupt::Interrupts, ExecutionWorker, FarPointer, Hook};

pub(super) async fn run(
    mut host: AppHost,
    events: EventLoopProxy<Event>,
    requests: Actions,
) -> Result<(), String> {
    let action = |action| {
        let (done, completed) = oneshot::channel();
        requests.borrow_mut().push_back((action, done));
        events.send_event(Event::Wake).unwrap();
        async { completed.await.unwrap() }
    };
    let (connected, endpoints) = oneshot::channel();
    let (ready, busy) = oneshot::channel();
    let (idle, waiting) = oneshot::channel();
    let result = ExecutionWorker::run_until(
        async {
            let (mouse, frames) = endpoints.await.unwrap();
            host.attach_mouse(mouse).unwrap();
            host.attach_display(move |device, _| {
                relocade_vga::VgaDisplay::new(device, frames).map_err(|error| error.to_string())
            })
            .unwrap();
            busy.await.unwrap();
            action(Action::Focus(true)).await;
            action(Action::Capture).await;
            #[cfg(target_arch = "wasm32")]
            wait_for(|| {
                web_sys::window()
                    .unwrap()
                    .document()
                    .unwrap()
                    .pointer_lock_element()
                    .is_some()
            })
            .await;
            action(Action::Button(ElementState::Released)).await;
            action(Action::Motion(8.0, 4.0)).await;
            action(Action::Button(ElementState::Pressed)).await;
            // INT 28h confirms that the guest has entered its DOS keyboard wait.
            waiting.await.unwrap();
            action(Action::Resize).await;
            action(Action::Motion(4.0, 2.0)).await;
            action(Action::Focus(false)).await;
            std::future::pending::<()>().await;
        },
        async move |runner| {
            let mut dos = Dos::new(runner).await.unwrap();
            let mut program = vec![
                0xb8, 0x13, 0, 0xcd, 0x10, // Mode 13h.
                0x31, 0xc0, 0xcd, 0x33, // Reset/detect.
                0xb8, 4, 0, 0xb9, 20, 0, 0xba, 30, 0, 0xcd, 0x33, // Position.
                0xb8, 15, 0, 0xb9, 8, 0, 0xba, 8, 0, 0xcd, 0x33, // Unity ratios.
                0xb8, 19, 0, 0xba, 0xff, 0xff, 0xcd, 0x33, // Disable acceleration here.
                0x0e, 0x07, // ES=CS for the far callback.
                0xb8, 12, 0, 0xb9, 7, 0, 0xba, 0, 3, 0xcd, 0x33, 0xb8, 1, 0, 0xcd,
                0x33, // Show the guest-VRAM cursor.
                0xeb, 0,
            ];
            let ready_offset = 0x100 + program.len() as u32;
            program.extend([
                0xeb, 0, // Host ready hook resumes at the polling loop.
                0xb8, 3, 0, 0xcd, 0x33, // Poll position/buttons while busy.
                0xf6, 0xc3, 1, 0x74, 0xf6, // Wait for the left button.
                0xb4, 7, 0xcd, 0x21, // Wait for a DOS key, supplied by the release callback.
                0xa2, 2, 5, // Log the key.
                0xb8, 3, 0, 0xcd, 0x33, 0x89, 0x1e, 4, 5, 0x89, 0x0e, 6, 5, 0x89, 0x16, 8, 5, 0xb8,
                11, 0, 0xcd, 0x33, 0x89, 0x0e, 10, 5, 0x89, 0x16, 12, 5, 0xb8, 0, 0x4c, 0xcd, 0x21,
            ]);
            program.resize(0x200, 0);
            program.extend([
                0x2e, 0x09, 0x06, 0, 5, // Accumulate callback event bits in CS:[0500].
                0xa8, 4, 0x74, 8, // Only release supplies the waiting key.
                0xb8, 0, 5, 0xb9, b'K', 0x25, 0xcd, 0x16,
                0xcb, // Far return using the ordinary interrupt resumption ABI.
            ]);
            dos.launch_com(&program, Default::default()).unwrap();
            let segment = dos.process().unwrap().psp_segment();
            let base = u32::from(segment) << 4;
            let state = dos.cpu().cpu_state();
            let memory = dos.cpu().memory();
            let ready = RefCell::new(Some(ready));
            dos.cpu()
                .install([Hook::host(base + ready_offset, async move || {
                    state.set_eip(ready_offset + 2);
                    ready.borrow_mut().take().unwrap().send(()).unwrap();
                })])
                .await
                .unwrap();
            let idle = RefCell::new(Some(idle));
            let mut interrupts = Interrupts::new(dos.cpu());
            interrupts.register_interrupt(
                0x28,
                FarPointer {
                    segment,
                    offset: 0x400,
                },
                async move |_| {
                    if let Some(idle) = idle.borrow_mut().take() {
                        let _ = idle.send(());
                    }
                },
            );
            interrupts.install().await.unwrap();
            connected
                .send((dos.mouse_input(), dos.frames()))
                .ok()
                .unwrap();
            assert_eq!(dos.run(runner).await, Ok(0));
            let view = memory.view();
            assert_eq!(view.read_u16(base + 0x500).unwrap() & 7, 7);
            assert_eq!(view.read_u8(base + 0x502).unwrap(), b'K');
            assert_eq!(
                view.read_u16(base + 0x504).unwrap(),
                0,
                "focus loss released the guest button"
            );
            assert_eq!(view.read_u16(base + 0x506).unwrap(), 32);
            assert_eq!(view.read_u16(base + 0x508).unwrap(), 36);
            assert_eq!(view.read_u16(base + 0x50a).unwrap(), 12);
            assert_eq!(view.read_u16(base + 0x50c).unwrap(), 6);
        },
    )
    .await
    .map_err(|error| error.to_string())?;
    assert!(
        result.is_some(),
        "guest finishes through its callback and DOS read"
    );
    Ok(())
}
