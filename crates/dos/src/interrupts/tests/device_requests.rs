use super::*;
use crate::devices::{attributes, CharacterDriver, DriverFuture};
use futures_channel::oneshot;
use std::{cell::RefCell, rc::Rc};

struct WaitingConsole {
    ready: RefCell<Option<oneshot::Receiver<()>>>,
    input: RefCell<&'static [u8]>,
    output: Rc<RefCell<Vec<u8>>>,
}

impl WaitingConsole {
    async fn wait(&self) {
        let ready = self.ready.borrow_mut().take();
        if let Some(ready) = ready {
            ready.await.unwrap();
        }
    }
}

impl CharacterDriver for WaitingConsole {
    fn name(&self) -> [u8; 8] {
        *b"CON     "
    }
    fn attributes(&self) -> u16 {
        attributes::CONSOLE
    }
    fn input_available(&self) -> DriverFuture<'_, bool> {
        Box::pin(async {
            self.wait().await;
            Ok(true)
        })
    }
    fn output_ready(&self) -> DriverFuture<'_, bool> {
        self.input_available()
    }
    fn flush_input(&self) -> DriverFuture<'_, ()> {
        Box::pin(async {
            self.wait().await;
            Ok(())
        })
    }
    fn read<'a>(&'a self, bytes: &'a mut [u8]) -> DriverFuture<'a, usize> {
        Box::pin(async move { Ok(self.input.borrow_mut().read(bytes).unwrap()) })
    }
    fn write<'a>(&'a self, bytes: &'a [u8]) -> DriverFuture<'a, usize> {
        Box::pin(async move {
            self.wait().await;
            self.output.borrow_mut().extend_from_slice(bytes);
            Ok(bytes.len())
        })
    }
}

#[test]
fn device_write_status_and_flush_can_suspend_the_current_dos_request() {
    pollster::block_on(async {
        for function in [0x4000, 0x4406, 0x4407, 0x0c00, 0x0c01, 0x0c0a] {
            let (clock, _timers) = timer_service();
            let (runner, _host, machine) = crate::test_runner::runner(clock.into()).await;
            let mut dos = runner
                .run(std::future::pending::<()>(), async |runner| {
                    Dos::new(runner).await.unwrap()
                })
                .await
                .unwrap();
            machine.shutdown().await.unwrap();
            dos.cpu().interrupt_handle().take();
            let (ready, wait) = oneshot::channel();
            let output = Rc::new(RefCell::new(Vec::new()));
            dos.state.devices().install(WaitingConsole {
                ready: RefCell::new(Some(wait)),
                input: RefCell::new(b"KL\r"),
                output: output.clone(),
            });
            dos.launch_com(&[0xcd, 0x21, 0xeb, 0xfe], Default::default())
                .unwrap();
            {
                let mut call = dos.cpu().context();
                call.cpu.set_eflags(2);
                call.cpu.set_ax(function);
                call.cpu.set_bx(1);
                call.cpu.set_cx(1);
                call.cpu.set_dx(0x300);
                call.memory.write(0x10300, b"A").unwrap();
                if function == 0x0c0a {
                    call.memory.write(0x10300, &[2, 0xcc]).unwrap();
                }
            }
            assert!(pin!(dos.cpu().run())
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending());
            assert!(dos.cpu().cpu_state().interrupts_enabled());
            // Pending requests release the handle table and retain their own
            // destination even if the process changes its handles.
            dos.process()
                .unwrap()
                .files()
                .force_duplicate(&mut dos.state.filesystem(), 3, 1)
                .unwrap();
            dos.process()
                .unwrap()
                .files()
                .force_duplicate(&mut dos.state.filesystem(), 3, 0)
                .unwrap();
            if function == 0x0c0a {
                // Buffered input starts after flushing and reads its buffer then.
                dos.cpu().memory().view().write_u8(0x10300, 3).unwrap();
            }
            ready.send(()).unwrap();
            assert_eq!(
                execution::run_until(dos.cpu(), 0x102).await,
                ExecutionExit::Interrupted
            );
            let call = dos.cpu().context();
            assert!(!call.cpu.carry());
            assert!(
                !call.cpu.interrupts_enabled(),
                "guest IRET restores caller IF"
            );
            match function {
                0x4000 => {
                    assert_eq!(call.cpu.ax(), 1);
                    assert_eq!(&*output.borrow(), b"A");
                }
                0x4406 | 0x4407 => assert_eq!(call.cpu.al(), 0xff),
                0x0c01 => {
                    assert_eq!(call.cpu.al(), b'K');
                    assert_eq!(&*output.borrow(), b"K");
                }
                0x0c00 => assert_eq!(call.cpu.al(), 0),
                0x0c0a => {
                    let mut buffer = [0; 5];
                    call.memory.read(0x10300, &mut buffer).unwrap();
                    assert_eq!(buffer, [3, 2, b'K', b'L', b'\r']);
                    assert_eq!(&*output.borrow(), b"KL\r");
                }
                _ => unreachable!(),
            }
        }
    });
}
