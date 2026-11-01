use crate::common::{self, path_service, service, success, SERVICE_PROGRAM};
use relocade_dos::{
    devices::{CharacterDriver, ReadFuture},
    DosError,
};
use std::{cell::RefCell, collections::VecDeque, rc::Rc};

struct ByteDevice {
    input: RefCell<VecDeque<u8>>,
    output: Rc<RefCell<Vec<u8>>>,
}

impl CharacterDriver for ByteDevice {
    fn name(&self) -> [u8; 8] {
        *b"BYTES   "
    }
    fn input_available(&self) -> bool {
        !self.input.borrow().is_empty()
    }
    fn try_read_character(&self) -> Result<Option<u8>, DosError> {
        Ok(self.input.borrow_mut().pop_front())
    }
    fn read_character(&self) -> ReadFuture<'_> {
        Box::pin(async { self.try_read_character()?.ok_or(DosError::AccessDenied) })
    }
    fn output_ready(&self) -> bool {
        true
    }
    fn write(&self, bytes: &[u8]) -> Result<(), DosError> {
        self.output.borrow_mut().extend_from_slice(bytes);
        Ok(())
    }
}

#[test]
fn installed_character_drivers_receive_handle_io_and_remain_bound_after_replacement() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        let first_output = Rc::new(RefCell::new(Vec::new()));
        dos.install_driver(ByteDevice {
            input: RefCell::new(b"ab".to_vec().into()),
            output: first_output.clone(),
        });
        let retained = dos.device(b"bytes").unwrap();
        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
        let first = success(&path_service(&mut dos, b"BYTES:", 0x3d02, 0));
        assert_eq!(success(&service(&mut dos, 0x4400, first, 0, 0)), 0x80c0);
        assert_eq!(success(&service(&mut dos, 0x4406, first, 0, 0)) as u8, 0xff);
        assert_eq!(success(&service(&mut dos, 0x3f00, first, 2, 0x400)), 2);
        let mut bytes = [0; 2];
        dos.cpu()
            .context()
            .memory
            .read(0x10400, &mut bytes)
            .unwrap();
        assert_eq!(&bytes, b"ab");
        assert_eq!(success(&service(&mut dos, 0x4406, first, 0, 0)) as u8, 0);

        let second_output = Rc::new(RefCell::new(Vec::new()));
        dos.install_driver(ByteDevice {
            input: RefCell::default(),
            output: second_output.clone(),
        });
        let second = success(&path_service(&mut dos, b"bytes.dat", 0x3d02, 0));
        let duplicate = success(&service(&mut dos, 0x4500, first, 0, 0));
        // Non-console drivers receive the original bytes, even on cooked handles.
        dos.cpu()
            .context()
            .memory
            .write(0x10400, b"A\t\x1aB")
            .unwrap();
        assert_eq!(success(&service(&mut dos, 0x4000, duplicate, 4, 0x400)), 4);
        assert_eq!(success(&service(&mut dos, 0x4000, second, 1, 0x403)), 1);
        retained.write(b"!").unwrap();
        assert_eq!(&*first_output.borrow(), b"A\t\x1aB!");
        assert_eq!(&*second_output.borrow(), b"B");
        assert!(common::output(dos.stdout()).is_empty());
    });
}

struct RejectingConsole(Rc<RefCell<VecDeque<u8>>>);

impl CharacterDriver for RejectingConsole {
    fn name(&self) -> [u8; 8] {
        *b"CON     "
    }
    fn attributes(&self) -> u16 {
        relocade_dos::devices::attributes::CONSOLE
    }
    fn read_character(&self) -> ReadFuture<'_> {
        Box::pin(async {
            self.0
                .borrow_mut()
                .pop_front()
                .ok_or(DosError::AccessDenied)
        })
    }
    fn write(&self, bytes: &[u8]) -> Result<(), DosError> {
        if bytes == b"\n" {
            Ok(())
        } else {
            Err(DosError::AccessDenied)
        }
    }
}

#[test]
fn driver_errors_reach_handle_and_rust_calls_while_dos_character_output_ignores_them() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        let input = Rc::new(RefCell::new(VecDeque::new()));
        dos.install_driver(RejectingConsole(input.clone()));
        assert_eq!(dos.console().write(b"X"), Err(DosError::AccessDenied));
        assert_eq!(dos.console().read_line().await, Err(DosError::AccessDenied));
        input.borrow_mut().extend(b"A\r");
        // Echo failure is returned even though this driver accepts the final LF.
        assert_eq!(dos.console().read_line().await, Err(DosError::AccessDenied));
        assert_eq!(&*input.borrow(), &VecDeque::from([b'\r']));

        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
        dos.cpu().context().memory.write(0x10400, b"X$").unwrap();
        common::error(&service(&mut dos, 0x4000, 1, 1, 0x400), 5);
        dos.cpu().context().cpu.set_eflags(0x202);
        assert_eq!(
            success(&service(&mut dos, 0x0200, 0, 0, u16::from(b'X'))),
            0x0258
        );
        assert_eq!(success(&service(&mut dos, 0x0900, 0, 0, 0x400)), 0x0924);

        input.borrow_mut().clear();
        input.borrow_mut().extend(b"B\r");
        dos.cpu()
            .context()
            .memory
            .write(0x10400, &[5, 0, 0, 0])
            .unwrap();
        service(&mut dos, 0x0a00, 0, 0, 0x400);
        let mut line = [0; 3];
        dos.cpu().context().memory.read(0x10401, &mut line).unwrap();
        assert_eq!(line, [1, b'B', b'\r']);
    });
}
