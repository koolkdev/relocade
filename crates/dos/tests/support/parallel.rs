use futures_io::AsyncRead;
use futures_util::future::join;
use relocade_clock_test_support::ManualClock;
use relocade_dos::{
    devices::{CharacterDriver, Printer},
    Dos, DosError, RunError,
};
use relocade_machine::{worker::wake_connection, yield_now, FarPointer, Hook, Runner};
use relocade_parallel::{centronics::Input, Centronics};
use relocade_pc::{Configuration, ParallelPort, Pc};
use relocade_x86::{CpuState, ExecutionExit, HostExit};
use std::{
    cell::RefCell,
    future::{poll_fn, Future},
    pin::{pin, Pin},
    rc::Rc,
    task::{Context, Poll, Waker},
    time::Duration,
};

fn base(dos: &Dos) -> u32 {
    u32::from(dos.process().unwrap().psp_segment()) << 4
}

fn prepare(dos: &mut Dos, vector: u8, ax: u16, bx: u16, cx: u16, dx: u16) {
    let address = base(dos);
    let mut call = dos.cpu().context();
    call.memory.write(address + 0x100, &[0xcd, vector]).unwrap();
    call.cpu.set_eip(0x100);
    call.cpu.set_sp(0x8000);
    call.cpu.set_ax(ax);
    call.cpu.set_bx(bx);
    call.cpu.set_cx(cx);
    call.cpu.set_dx(dx);
    call.cpu.set_eflags(0x202);
}

async fn finish(dos: &mut Dos) -> CpuState {
    assert_eq!(
        dos.run().await,
        Err(RunError::Execution(ExecutionExit::Host(HostExit::Return)))
    );
    dos.cpu().context().cpu.snapshot()
}

async fn service(dos: &mut Dos, ax: u16, bx: u16, cx: u16, dx: u16) -> CpuState {
    prepare(dos, 0x21, ax, bx, cx, dx);
    finish(dos).await
}

fn success(state: CpuState) -> u16 {
    assert_eq!(state.flags.eflags() & 1, 0);
    state.registers.eax as u16
}

fn error(state: CpuState, expected: u16) {
    assert_eq!(state.flags.eflags() & 1, 1);
    assert_eq!(state.registers.eax as u16, expected);
}

async fn open(dos: &mut Dos, name: &[u8], mode: u8) -> u16 {
    let mut path = name.to_vec();
    path.push(0);
    let address = base(dos);
    dos.cpu()
        .memory()
        .view()
        .write(address + 0x300, &path)
        .unwrap();
    success(service(dos, 0x3d00 | u16::from(mode), 0, 0, 0x300).await)
}

async fn read(input: &mut Input, count: usize) -> Vec<u8> {
    let mut bytes = Vec::new();
    for _ in 0..count {
        let mut byte = [0];
        let read = poll_fn(|cx| Pin::new(&mut *input).poll_read(cx, &mut byte))
            .await
            .unwrap();
        assert_eq!(read, 1);
        bytes.push(byte[0]);
    }
    bytes
}

fn pending(dos: &mut Dos) {
    for _ in 0..16 {
        match pin!(dos.cpu().run()).poll(&mut Context::from_waker(Waker::noop())) {
            Poll::Pending => return,
            Poll::Ready(ExecutionExit::Interrupted) => {}
            result => panic!("expected a suspended LPT write, got {result:?}"),
        }
    }
    panic!("DOS did not reach its LPT wait");
}

struct Replacement {
    name: [u8; 8],
    output: Rc<RefCell<Vec<u8>>>,
}

impl CharacterDriver for Replacement {
    fn name(&self) -> [u8; 8] {
        self.name
    }

    fn write<'a>(&'a self, bytes: &'a [u8]) -> relocade_dos::devices::DriverFuture<'a, usize> {
        Box::pin(async move {
            self.output.borrow_mut().extend_from_slice(bytes);
            Ok(bytes.len())
        })
    }
}

async fn exercise(runner: Runner, clock: &ManualClock, number: u8) {
    let pc = Pc::with_configuration(
        runner,
        Configuration {
            parallel_port: Some(ParallelPort {
                base: 0x278,
                irq: 5,
            }),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let port = pc.parallel_port().unwrap();
    let (mut input, output) = Centronics::connect(pc.runner(), &port).await;
    let mut dos = Dos::from_pc(pc).await.unwrap();
    dos.bind_lpt(number, output.clone()).unwrap();
    // A numbered LPT binding leaves PRN unavailable until its driver is installed.
    let unbound_printer = dos.device(b"PRN").unwrap();
    assert_eq!(unbound_printer.name(), *b"PRN     ");
    assert!(!unbound_printer.output_ready().await.unwrap());
    assert_eq!(unbound_printer.write(b"X").await, Err(DosError::AccessDenied));
    if number == 1 {
        dos.install_driver(Printer::new(output.clone()));
    }
    dos.launch_com(&[0xcd, 0x21], Default::default()).unwrap();
    let address = base(&dos);
    let exit_address = FarPointer {
        segment: dos.process().unwrap().psp_segment(),
        offset: 0x102,
    };
    let memory = dos.cpu().memory();
    let io = dos.cpu().context().io.clone();
    let exit = dos.cpu().exit_handle();
    dos.cpu()
        .install([Hook::host(exit_address.packed(), async move || {
            exit.request();
        })])
        .await
        .unwrap();
    let name = format!("lpt{number}.dat:");
    let handle = open(&mut dos, name.as_bytes(), 2).await;
    let retained = dos.device(format!("LPT{number}").as_bytes()).unwrap();
    assert_eq!(
        success(service(&mut dos, 0x4400, handle, 0, 0).await),
        0x80c0
    );
    assert_eq!(
        success(service(&mut dos, 0x4407, handle, 0, 0).await) as u8,
        0xff
    );
    assert!(dos.read_file(&format!("LPT{number}.DAT")).is_none());
    let readonly = open(&mut dos, name.as_bytes(), 0).await;
    error(service(&mut dos, 0x4000, readonly, 1, 0x400).await, 5);
    success(service(&mut dos, 0x3e00, readonly, 0, 0).await);
    let unbound = open(&mut dos, b"LPT2", 1).await;
    assert_eq!(
        success(service(&mut dos, 0x4407, unbound, 0, 0).await) as u8,
        0
    );
    error(service(&mut dos, 0x4000, unbound, 1, 0x400).await, 5);
    success(service(&mut dos, 0x3e00, unbound, 0, 0).await);
    let prn = open(&mut dos, b"PRN", 2).await;
    if number == 1 {
        // PRN and process startup handle 4 use the explicitly installed driver.
        error(service(&mut dos, 0x3f00, prn, 1, 0x400).await, 5);
        memory.view().write(address + 0x400, b"\t\x1a").unwrap();
        for destination in [prn, 4] {
            assert_eq!(
                success(service(&mut dos, 0x4407, destination, 0, 0).await) as u8,
                0xff
            );
            prepare(&mut dos, 0x21, 0x4000, destination, 2, 0x400);
            pending(&mut dos);
            let (state, received) = join(finish(&mut dos), read(&mut input, 2)).await;
            assert_eq!(success(state), 2);
            assert_eq!(received, b"\t\x1a");
        }
    } else {
        error(service(&mut dos, 0x4000, prn, 1, 0x400).await, 5);
        error(service(&mut dos, 0x4000, 4, 1, 0x400).await, 5);
        success(service(&mut dos, 0x3e00, prn, 0, 0).await);
    }

    // The numbered DOS binding is explicit and independent of the
    // hardware address or first BIOS slot used by this fixture.
    io.write_port(0x278, 1, 0xa5);
    io.write_port(0x27a, 1, 0x0e);
    let original_int17 = memory.view().read_u32(0x17 * 4).unwrap();
    // An installed INT 17h hook counts calls; direct DOS output must bypass it.
    let int17_hook = FarPointer {
        offset: 0x180,
        ..exit_address
    };
    memory
        .view()
        .write(int17_hook.address(), &[0x2e, 0xfe, 6, 0, 5, 0xcf])
        .unwrap();
    memory.view().write_u8(address + 0x500, 0).unwrap();
    memory
        .view()
        .write_u32(0x17 * 4, int17_hook.packed())
        .unwrap();
    let payload = [0, 9, 0x1a, 0xff];
    memory.view().write(address + 0x400, &payload).unwrap();
    prepare(&mut dos, 0x21, 0x4000, handle, 4, 0x400);
    pending(&mut dos);
    assert!(!retained.output_ready().await.unwrap());
    assert!(dos.cpu().cpu_state().interrupts_enabled());
    assert_eq!(io.read_port(0x278, 1), 0xa5);
    assert_eq!(io.read_port(0x27a, 1), 0xee);
    // A suspended call retains its original bytes and driver.
    memory.view().write(address + 0x400, b"edit").unwrap();
    let replacement = Rc::new(RefCell::new(Vec::new()));
    if number == 1 {
        dos.install_driver(Replacement {
            name: *b"LPT1    ",
            output: replacement.clone(),
        });
    }
    let (state, received) = join(finish(&mut dos), read(&mut input, 4)).await;
    assert_eq!(success(state), 4);
    assert_eq!(received, payload);
    assert!(replacement.borrow().is_empty());
    assert_eq!(io.read_port(0x278, 1), 0xa5);
    assert_eq!(io.read_port(0x27a, 1), 0xee);
    assert!(retained.output_ready().await.unwrap());
    if number == 1 {
        // AH=05h retains standard handle 4 despite the new LPT1 registration.
        for byte in payload {
            let state = service(&mut dos, 0x05aa, 0x1234, 0x5678, u16::from(byte)).await;
            assert_eq!(success(state), 0x0500 | u16::from(byte));
            assert_eq!(read(&mut input, 1).await, [byte]);
        }
        // The second byte suspends until the first is consumed, without
        // changing the port's data/control registers or calling INT 17h.
        retained.write(b"P").await.unwrap();
        prepare(&mut dos, 0x21, 0x05aa, 0x1234, 0x5678, 0x9abc);
        dos.cpu().cpu_state().set_eax(0xdead05aa);
        dos.cpu().cpu_state().set_eflags(0x647);
        pending(&mut dos);
        assert!(dos.cpu().cpu_state().interrupts_enabled());
        let (state, received) = join(finish(&mut dos), read(&mut input, 2)).await;
        assert_eq!(received, b"P\xbc");
        assert_eq!(state.registers.eax, 0xdead05bc);
        assert_eq!(state.registers.ebx as u16, 0x1234);
        assert_eq!(state.registers.ecx as u16, 0x5678);
        assert_eq!(state.registers.edx as u16, 0x9abc);
        assert_eq!(state.flags.eflags(), 0x647);
        assert!(replacement.borrow().is_empty());
        assert_eq!(io.read_port(0x278, 1), 0xa5);
        assert_eq!(io.read_port(0x27a, 1), 0xee);
    }
    assert_eq!(memory.view().read_u8(address + 0x500).unwrap(), 0);
    memory.view().write_u32(0x17 * 4, original_int17).unwrap();

    if number == 3 {
        // PRN can explicitly share LPT3's connection while LPT1 is unavailable.
        let lpt1 = open(&mut dos, b"LPT1", 1).await;
        error(service(&mut dos, 0x4000, lpt1, 1, 0x400).await, 5);
        dos.install_driver(Printer::new(output));
        let installed_prn = open(&mut dos, b"prn.dat:", 1).await;
        assert_eq!(
            success(service(&mut dos, 0x4000, installed_prn, 1, 0x400).await),
            1
        );
        assert_eq!(read(&mut input, 1).await, b"e");
        // Handle 4 was opened before installation and retains the unavailable driver.
        error(service(&mut dos, 0x4000, 4, 1, 0x400).await, 5);
        return;
    }
    let latest = open(&mut dos, b"LPT1", 1).await;
    assert_eq!(
        success(service(&mut dos, 0x4000, latest, 1, 0x400).await),
        1
    );
    assert_eq!(&*replacement.borrow(), b"e");
    success(service(&mut dos, 0x3e00, latest, 0, 0).await);

    let current_prn = open(&mut dos, b"prn.dat:", 1).await;
    assert_eq!(
        success(service(&mut dos, 0x4000, current_prn, 1, 0x400).await),
        1
    );
    // Even a fresh PRN open retains the configured connection after LPT1 changes.
    assert_eq!(read(&mut input, 1).await, b"e");
    assert_eq!(&*replacement.borrow(), b"e");
    let replacement_printer = Rc::new(RefCell::new(Vec::new()));
    dos.install_driver(Replacement {
        name: *b"PRN     ",
        output: replacement_printer.clone(),
    });
    let latest_prn = open(&mut dos, b"PRN", 1).await;
    assert_eq!(
        success(service(&mut dos, 0x4000, latest_prn, 1, 0x400).await),
        1
    );
    assert_eq!(&*replacement_printer.borrow(), b"e");
    for destination in [prn, current_prn, 4] {
        assert_eq!(
            success(service(&mut dos, 0x4000, destination, 1, 0x400).await),
            1
        );
        assert_eq!(read(&mut input, 1).await, b"e");
    }
    for destination in [prn, current_prn, latest_prn] {
        success(service(&mut dos, 0x3e00, destination, 0, 0).await);
    }
    // Existing handles and their duplicates still use the shared stream.
    let duplicate = success(service(&mut dos, 0x4500, handle, 0, 0).await);
    success(service(&mut dos, 0x3e00, handle, 0, 0).await);
    error(service(&mut dos, 0x4000, handle, 1, 0x400).await, 6);
    error(service(&mut dos, 0x3f00, duplicate, 1, 0x400).await, 5);
    assert_eq!(
        success(service(&mut dos, 0x4000, duplicate, 0, 0x400).await),
        0
    );
    let saved_stdout = success(service(&mut dos, 0x4500, 1, 0, 0).await);
    success(service(&mut dos, 0x4600, duplicate, 1, 0).await);
    prepare(&mut dos, 0x21, 0x4000, 1, 2, 0x400);
    let (state, received) = join(finish(&mut dos), read(&mut input, 2)).await;
    assert_eq!(success(state), 2);
    assert_eq!(received, b"ed");
    success(service(&mut dos, 0x4600, saved_stdout, 1, 0).await);
    success(service(&mut dos, 0x3e00, saved_stdout, 0, 0).await);
    let mut captured = Vec::new();
    std::io::Read::read_to_end(&mut dos.stdout(), &mut captured).unwrap();
    assert!(captured.is_empty());

    // Real guest OUT and INT 17h still reach the original reader.
    memory
        .view()
        .write(
            address + 0x110,
            &[
                0xba, 0x78, 2, 0xb0, 0x1b, 0xee, // Data latch = ESC.
                0xba, 0x7a, 2, 0xb0, 0x0d, 0xee, // Assert STROBE.
                0xb0, 0x0c, 0xee, 0xe9, 0xe0, 0xff, // Release and jump to exit.
            ],
        )
        .unwrap();
    dos.cpu().cpu_state().set_eip(0x110);
    finish(&mut dos).await;
    assert_eq!(read(&mut input, 1).await, [0x1b]);
    clock.advance_by(Duration::from_micros(5));
    yield_now().await;
    prepare(&mut dos, 0x17, 0x005a, 0, 0, 0);
    assert_eq!(finish(&mut dos).await.registers.eax as u16, 0x105a);
    assert_eq!(read(&mut input, 1).await, [0x5a]);
    // DOS waits for the active ACK without synthesizing another strobe.
    prepare(&mut dos, 0x21, 0x4000, duplicate, 1, 0x400);
    pending(&mut dos);
    clock.advance_by(Duration::from_micros(5));
    let (state, received) = join(finish(&mut dos), read(&mut input, 1)).await;
    assert_eq!(success(state), 1);
    assert_eq!(received, b"e");
    assert_eq!(io.read_port(0x278, 1), 0x5a);
    assert_eq!(io.read_port(0x27a, 1), 0xec);
    drop(input);
    for destination in [duplicate, 4] {
        assert_eq!(
            success(service(&mut dos, 0x4407, destination, 0, 0).await) as u8,
            0
        );
        error(
            service(&mut dos, 0x4000, destination, 1, 0x400).await,
            DosError::WriteFault.code(),
        );
    }
    // Unlike AH=40h, the legacy printer call has no error result on reader loss.
    assert_eq!(success(service(&mut dos, 0x05aa, 0, 0, 0x7f).await), 0x057f);
}

pub async fn parallel_file_access() -> Result<(), String> {
    for number in [1, 3] {
        let clock = ManualClock::default();
        let mut machine =
            relocade_machine::Machine::new(relocade_pc::configuration::memory()).unwrap();
        let cpu = machine
            .create_cpu(relocade_pc::configuration::cpu(), None)
            .unwrap();
        let (_host, endpoint) = wake_connection(cpu.interrupt_handle());
        let runner = Runner::new(clock.clock(), cpu, endpoint).await.unwrap();
        runner
            .run(
                futures_timer::Delay::new(Duration::from_secs(30)),
                async |runner| exercise(runner, &clock, number).await,
            )
            .await
            .ok_or_else(|| "DOS parallel file access timed out".to_string())?;
        machine.shutdown().await.unwrap();
    }
    Ok(())
}
