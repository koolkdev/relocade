use super::*;
use crate::devices::{attributes, CharacterDriver, DriverFuture};
use futures_channel::oneshot;
use std::{cell::RefCell, rc::Rc};

struct WaitingConsole {
    ready: RefCell<Option<oneshot::Receiver<()>>>,
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
        Box::pin(async move {
            bytes.fill(b'K');
            Ok(bytes.len())
        })
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
        for function in [0x4000, 0x4406, 0x4407, 0x0c00, 0x0c01] {
            let (clock, _timers) = timer_service();
            let mut dos = Dos::new(&Runner::with_clock(clock)).await.unwrap();
            let (ready, wait) = oneshot::channel();
            let output = Rc::new(RefCell::new(Vec::new()));
            dos.state.devices().install(WaitingConsole {
                ready: RefCell::new(Some(wait)),
                output: output.clone(),
            });
            dos.launch_com(&[0xcd, 0x21, 0xeb, 0xfe], Default::default())
                .unwrap();
            {
                let mut call = dos.cpu().context();
                call.cpu.set_ax(function);
                call.cpu.set_bx(1);
                call.cpu.set_cx(1);
                call.cpu.set_dx(0x300);
                call.memory.write(0x10300, b"A").unwrap();
            }
            assert!(pin!(dos.cpu().run(10))
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending());
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
            ready.send(()).unwrap();
            assert_eq!(dos.cpu().run(10).await, ExecutionExit::BudgetExhausted);
            let call = dos.cpu().context();
            assert!(!call.cpu.carry());
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
                _ => unreachable!(),
            }
        }
    });
}
