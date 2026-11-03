use super::*;
use crate::{com, files::Handle, kernel, state::DosState, stdio::OutputStream};
use relocade_machine::{Cpu, Device, DeviceConnection, IoBus, Memory};
use relocade_vga::Vga;
use relocade_x86::Segment;
use std::{cell::RefCell, rc::Rc, task::Poll};

#[derive(Default)]
struct Irq {
    pending: bool,
    acknowledged: usize,
    waker: Option<Waker>,
}

impl Irq {
    fn raise(&mut self) {
        self.pending = true;
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }
}

impl Device for Irq {
    fn poll_interrupt(&mut self, cx: &mut Context<'_>) -> Poll<u8> {
        if std::mem::take(&mut self.pending) {
            self.acknowledged += 1;
            Poll::Ready(8)
        } else {
            self.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}

/// Use the production loader, DOS services and BIOS resources with a controlled IRQ source.
async fn session(irq: &Rc<RefCell<Irq>>) -> (Cpu, Rc<DosState>) {
    let (clock, _timers) = timer_service();
    let io = IoBus::new([
        DeviceConnection {
            device: Rc::new(RefCell::new(Vga::with_clock(Rc::new(clock)))),
            mmio: vec![relocade_vga::MEMORY_RANGE],
            ports: relocade_vga::PORT_RANGES.into_iter().collect(),
        },
        DeviceConnection {
            device: irq.clone(),
            mmio: vec![],
            ports: vec![],
        },
    ]);
    let memory = Memory::new(kernel::configuration()).unwrap();
    let mut cpu = Cpu::new(memory.clone(), kernel::cpu(), io.clone())
        .await
        .unwrap();
    let keyboard = relocade_bios::Keyboard::default();
    let dos = Rc::new(DosState::new(
        memory.clone().with_io(io.clone()),
        keyboard.clone(),
        cpu.exit_handle(),
    ));
    crate::memory::reset(&mut cpu.context().memory);
    relocade_bios::initialize(&mut cpu.context().memory, &io);
    let mut interrupts = relocade_machine::real_mode_interrupt::Interrupts::new(&mut cpu);
    crate::interrupt::register_interrupts(&mut interrupts, dos.clone());
    relocade_bios::register_interrupts(&mut interrupts, keyboard);
    interrupts.install().await.unwrap();
    let psp = com::load(
        &mut cpu.context(),
        &[0xcd, 0x21, 0xeb, 0xfe],
        &Default::default(),
    )
    .unwrap();
    dos.start_process(psp);
    cpu.cpu_state().set_eflags(2);
    memory.view().write(8 * 4, &[0, 3, 0, 0]).unwrap();
    // Jump ends the first block so a budget can park execution inside the ISR.
    // AH=46h replaces stdout with stderr, then stdin with AUX. Neither replacement
    // may redirect the outer request's already captured resources.
    memory
        .view()
        .write(
            0x300,
            &[
                0xb8, 0, 0x46, 0xeb, 0, // MOV AX,4600h; JMP next.
                0xbb, 2, 0, 0xb9, 1, 0, 0xcd, 0x21, // duplicate stderr to stdout.
                0xb8, 0, 0x46, 0xbb, 3, 0, 0xb9, 0, 0, 0xcd, 0x21, // AUX to stdin.
                0xb8, 0x22, 0x22, 0x8e, 0xd8, // MOV AX,2222h; MOV DS,AX.
                0xbb, 0xef, 0xbe, 0xb9, 0x34, 0x12, 0xba, 0, 5, // BX,CX,DX effects.
                0xcf,
            ],
        )
        .unwrap();
    (cpu, dos)
}

fn pending(future: impl Future) {
    assert!(pin!(future)
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
        .is_pending());
}

#[test]
fn irq_reentry_preserves_dos_input_resources_and_arguments() {
    pollster::block_on(async {
        for function in [0x01, 0x07, 0x08, 0x0a, 0x3f] {
            let irq = Rc::new(RefCell::new(Irq::default()));
            let (mut cpu, dos) = session(&irq).await;
            let state = cpu.cpu_state();
            let memory = cpu.memory();
            let buffer = state.segment(Segment::Ds).base + 0x300;
            memory
                .view()
                .write(buffer, &[8, 0, 0xcc, 0xcc, 0xcc, 0xcc])
                .unwrap();
            state.set_ah(function);
            state.set_bx(0);
            state.set_cx(2);
            state.set_dx(0x300);
            let mut input = dos.stdio().input.clone();
            if function == 0x3f {
                dos.process()
                    .unwrap()
                    .files()
                    .set_device_information(0, 0x60)
                    .unwrap();
                input.write_all(b"A").unwrap(); // Retain a partial raw transfer.
            }
            pending(cpu.run(100));
            assert!(state.interrupts_enabled());
            irq.borrow_mut().raise();
            assert_eq!(cpu.run(1).await, ExecutionExit::BudgetExhausted);
            assert_eq!(state.eip(), 0x305);
            input
                .write_all(match function {
                    0x0a => b"AB\r",
                    0x3f => b"BC",
                    _ => b"K",
                })
                .unwrap();
            // The next block enters nested INT 21h. Waking the outer input must
            // not poll it before the guest returns to its Resume entry.
            assert_eq!(cpu.run(1).await, ExecutionExit::BudgetExhausted);
            assert!(input.peek_key().is_some());
            assert_eq!(cpu.run(100).await, ExecutionExit::BudgetExhausted);
            {
                let process = dos.process().unwrap();
                let files = process.files();
                for (source, target) in [(2, 1), (3, 0)] {
                    let (Handle::Device(source), Handle::Device(target)) =
                        (files.handle(source).unwrap(), files.handle(target).unwrap())
                    else {
                        panic!("the IRQ duplicates character-device handles");
                    };
                    assert!(
                        Rc::ptr_eq(source, target),
                        "nested AH=46h replaced the slot"
                    );
                }
            }
            assert_eq!(irq.borrow().acknowledged, 1);
            assert_eq!(state.segment(Segment::Ds).selector, 0x2222);
            assert_eq!(
                (state.bx(), state.cx(), state.dx()),
                (0xbeef, 0x1234, 0x500)
            );
            assert!(!state.interrupts_enabled());
            assert!(!state.carry());
            match function {
                0x0a => {
                    let mut bytes = [0; 6];
                    memory.view().read(buffer, &mut bytes).unwrap();
                    assert_eq!(bytes, [8, 2, b'A', b'B', b'\r', 0xcc]);
                }
                0x3f => {
                    let mut bytes = [0; 4];
                    memory.view().read(buffer, &mut bytes).unwrap();
                    assert_eq!(bytes, [b'A', b'B', 0xcc, 0xcc]);
                    assert_eq!(state.ax(), 2);
                    assert_eq!(input.try_read_key().unwrap().character, b'C');
                }
                _ => assert_eq!(state.al(), b'K'),
            }
            assert_eq!(input.peek_key(), None);
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            dos.stdio()
                .output(OutputStream::Stdout)
                .clone()
                .read_to_end(&mut stdout)
                .unwrap();
            dos.stdio()
                .output(OutputStream::Stderr)
                .clone()
                .read_to_end(&mut stderr)
                .unwrap();
            assert_eq!(
                stdout,
                match function {
                    0x01 => b"K".as_slice(),
                    0x0a => b"AB\r".as_slice(),
                    _ => b"".as_slice(),
                }
            );
            assert!(stderr.is_empty(), "echo retains the original stdout");
        }
    });
}

#[test]
fn ready_dos_input_finishes_without_delivering_an_irq() {
    pollster::block_on(async {
        let irq = Rc::new(RefCell::new(Irq::default()));
        let (mut cpu, dos) = session(&irq).await;
        cpu.cpu_state().set_ah(0x07);
        dos.stdio().input.clone().write_all(b"K").unwrap();
        irq.borrow_mut().raise();
        assert_eq!(cpu.run(100).await, ExecutionExit::BudgetExhausted);
        assert_eq!(cpu.cpu_state().al(), b'K');
        assert!(!cpu.cpu_state().interrupts_enabled());
        assert_eq!(irq.borrow().acknowledged, 0);
    });
}
