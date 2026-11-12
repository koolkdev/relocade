#[path = "../../machine/tests/support/execution.rs"]
mod execution;
use relocade_bios::{KeyMode, KeyStroke, Keyboard};
use relocade_machine::{
    real_mode_interrupt::Interrupts, Cpu, CpuConfig, Device, IoBus, IoContext, MachineConfig,
    Memory,
};
use relocade_vga::DispiAdapter;
use relocade_x86::{
    Compiler, CpuState, ExecutionExit, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap,
    Segment,
};
use std::{
    cell::RefCell,
    future::Future,
    pin::pin,
    rc::Rc,
    task::{Context, Poll, Waker},
};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

const KEY: KeyStroke = KeyStroke {
    character: b'K',
    scan_code: 0x25,
};

/// Controlled timer edges; the interrupt handler advances BDA time.
#[derive(Default)]
struct Timer {
    pending: bool,
    waker: Option<Waker>,
    acknowledged: usize,
}

impl Timer {
    fn tick(&mut self) {
        self.pending = true;
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }
}

impl Device for Timer {
    fn poll_interrupt(&mut self, _context: IoContext<'_>, cx: &mut Context<'_>) -> Poll<u8> {
        if std::mem::take(&mut self.pending) {
            self.acknowledged += 1;
            Poll::Ready(8)
        } else {
            self.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}

async fn machine(timer: &Rc<RefCell<Timer>>) -> (Cpu, Keyboard, Rc<RefCell<DispiAdapter>>) {
    let map = PhysicalMemoryMap::new([
        (0..=0x9ffff, PhysicalMapping::Ram { backing_offset: 0 }),
        (relocade_vga::MEMORY_RANGE, PhysicalMapping::Mmio),
        (
            0xf0000..=0xfffff,
            PhysicalMapping::Rom {
                backing_offset: 0xf0000,
            },
        ),
    ])
    .unwrap();
    let memory = Memory::new(MachineConfig::new(0x100000, map).unwrap()).unwrap();
    let keyboard = Keyboard::new(memory.clone());
    let (clock, _timers) = relocade_machine::timer_service();
    let vga = Rc::new(RefCell::new(DispiAdapter::with_clock(clock.into())));
    let io = IoBus::default();
    io.connect(vga.clone(), |io| DispiAdapter::configure_io(io, &vga));
    io.connect(timer.clone(), |_| {});
    let mut state = CpuState {
        eip: 0x100,
        ..CpuState::real_mode()
    };
    state.registers.esp = 0x8000;
    state.flags.set_eflags(2); // The caller has IF clear, independently of INT entry.
    let config = CpuConfig::new(Compiler::new(ExecutionProfile::Real16).target(), state).unwrap();
    let mut cpu = Cpu::new(memory.clone(), config, io.clone()).await.unwrap();
    relocade_bios::initialize(
        &mut memory.view().with_io(&io),
        &io,
        relocade_bios::Configuration {
            conventional_memory_kib: 640,
            math_coprocessor: true,
            pointing_device: false,
        },
    );
    let mut interrupts = Interrupts::new(&mut cpu);
    relocade_bios::register_interrupts(&mut interrupts, keyboard.clone(), vga.clone(), None);
    interrupts.install().await.unwrap();
    memory.view().write(8 * 4, &[0, 3, 0, 0]).unwrap();
    // INT 16h; save AX; INT 1Ah/AH=00h; save DX; loop.
    memory
        .view()
        .write(
            0x100,
            &[
                0xcd, 0x16, 0xa3, 0, 5, 0x31, 0xc0, 0xcd, 0x1a, 0x89, 0x16, 2, 5, 0xeb, 0xfe,
            ],
        )
        .unwrap();
    // IRQ0: INC word [046Ch]; INT 1Ah/AH=00h; save DX; change BX; IRET.
    // Short jumps delimit blocks so checkpoints can stop before nested BIOS entry
    // and IRQ return without depending on straight-line instruction granularity.
    memory
        .view()
        .write(
            0x300,
            &[
                0xff, 0x06, 0x6c, 4, 0xeb, 0, 0xb4, 0, 0xcd, 0x1a, 0x89, 0x16, 4, 5, 0xbb, 0x21,
                0x43, 0xeb, 0, 0xcf,
            ],
        )
        .unwrap();
    (cpu, keyboard, vga)
}

fn pending(future: impl Future) {
    assert!(pin!(future)
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
        .is_pending());
}

async fn clock_during_wait() {
    let timer = Rc::new(RefCell::new(Timer::default()));
    let (mut cpu, keyboard, _) = machine(&timer).await;
    let state = cpu.cpu_state();
    let memory = cpu.memory();
    let entry = memory.view().read_u32(0x16 * 4).unwrap();
    pending(cpu.run());
    assert!(state.interrupts_enabled());
    timer.borrow_mut().tick();
    assert_eq!(
        execution::checkpoint(&mut cpu).await,
        ExecutionExit::Interrupted
    );
    assert_eq!(state.eip(), 0x306);
    assert_eq!(memory.view().read_u16(0x7ff4).unwrap(), entry as u16 + 1);
    assert_eq!(
        memory.view().read_u16(0x7ff6).unwrap(),
        (entry >> 16) as u16
    );
    assert_ne!(memory.view().read_u16(0x7ff8).unwrap() & 0x200, 0);
    pending(cpu.run());
    assert_eq!(memory.view().read_u32(0x46c).unwrap(), 1);
    assert_eq!(memory.view().read_u16(0x504).unwrap(), 1);

    timer.borrow_mut().tick();
    assert_eq!(
        execution::checkpoint(&mut cpu).await,
        ExecutionExit::Interrupted
    );
    assert!(keyboard.store_key(KEY));
    // Finish the nested clock call, then the IRQ's IRET. Readiness alone must not
    // consume the key while the guest ISR or its nested BIOS call owns execution.
    assert_eq!(
        execution::run_until(&mut cpu, 0x30a).await,
        ExecutionExit::Interrupted
    );
    assert_eq!(state.eip(), 0x30a);
    assert_eq!(keyboard.peek_key(KeyMode::Enhanced), Some(KEY));
    assert_eq!(
        execution::run_until(&mut cpu, 0x313).await,
        ExecutionExit::Interrupted
    );
    assert_eq!(state.eip(), 0x313);
    assert_eq!(keyboard.peek_key(KeyMode::Enhanced), Some(KEY));
    assert_eq!(
        execution::checkpoint(&mut cpu).await,
        ExecutionExit::Interrupted
    );
    assert_eq!(state.segment(Segment::Cs).selector, (entry >> 16) as u16);
    assert_eq!(state.eip(), u32::from(entry as u16 + 1));
    assert_eq!(keyboard.peek_key(KeyMode::Enhanced), Some(KEY));

    assert_eq!(
        execution::run_until(&mut cpu, 0x10d).await,
        ExecutionExit::Interrupted
    );
    assert_eq!(keyboard.peek_key(KeyMode::Enhanced), None);
    assert_eq!(memory.view().read_u16(0x500).unwrap(), 0x254b);
    assert_eq!(memory.view().read_u16(0x502).unwrap(), 2);
    assert_eq!(memory.view().read_u16(0x504).unwrap(), 2);
    assert_eq!(timer.borrow().acknowledged, 2);
    assert_eq!(state.bx(), 0x4321, "guest ISR effects remain live");
    assert_eq!(state.sp(), 0x8000);
    assert!(!state.interrupts_enabled(), "IRET restores the caller's IF");
}

async fn ready_services() {
    let timer = Rc::new(RefCell::new(Timer::default()));
    let (mut cpu, keyboard, _) = machine(&timer).await;
    // After the keyboard read and clock query, query video state before looping.
    cpu.memory()
        .view()
        .write(0x10d, &[0xb4, 0x0f, 0xcd, 0x10, 0xeb, 0xfe])
        .unwrap();
    assert!(keyboard.store_key(KEY));
    timer.borrow_mut().tick();
    assert_eq!(
        execution::run_until(&mut cpu, 0x111).await,
        ExecutionExit::Interrupted
    );
    assert_eq!(cpu.memory().view().read_u16(0x500).unwrap(), 0x254b);
    assert_eq!(
        timer.borrow().acknowledged,
        0,
        "ready keyboard, clock and video services finish in one poll"
    );
    assert_eq!(cpu.cpu_state().eip(), 0x111);
    assert!(!cpu.cpu_state().interrupts_enabled());
}

async fn abandon_during_irq() {
    let timer = Rc::new(RefCell::new(Timer::default()));
    let (mut cpu, keyboard, _) = machine(&timer).await;
    let state = cpu.cpu_state();
    // STI; JMP to self. Live ISR IF must differ from the outer wrapper's saved IF.
    cpu.memory()
        .view()
        .write(0x304, &[0xfb, 0xeb, 0xfe])
        .unwrap();
    pending(cpu.run());
    timer.borrow_mut().tick();
    assert_eq!(
        execution::checkpoint(&mut cpu).await,
        ExecutionExit::Interrupted
    );
    assert!(keyboard.store_key(KEY));
    let before = state.snapshot();
    assert!(state.interrupts_enabled());
    drop(cpu);
    assert_eq!(state.snapshot(), before);
    assert_eq!(keyboard.peek_key(KeyMode::Enhanced), Some(KEY));
}

async fn bios_timer_rollover() {
    let timer = Rc::new(RefCell::new(Timer::default()));
    let (mut cpu, keyboard, vga) = machine(&timer).await;
    // Restore BIOS vectors instead of the fixture's guest IRQ0 handler.
    let mut interrupts = Interrupts::new(&mut cpu);
    relocade_bios::register_interrupts(&mut interrupts, keyboard.clone(), vga, None);
    interrupts.install().await.unwrap();
    let memory = cpu.memory();
    memory.view().write(0x46c, &[0xaf, 0, 0x18, 0, 1]).unwrap();
    let state = cpu.cpu_state();
    state.set_ax(0x55);
    state.set_bx(0x1234);
    state.set_cx(0x5678);
    state.set_dx(0x9abc);
    state.set_eflags(0x443);
    pending(cpu.run());
    let before = state.snapshot();
    timer.borrow_mut().tick();
    pending(cpu.run());
    let after = state.snapshot();
    assert_eq!(after.registers, before.registers);
    assert_eq!(after.segments, before.segments);
    assert_eq!(after.flags.eflags(), before.flags.eflags());
    assert_eq!(after.eip, before.eip);
    assert_eq!(memory.view().read_u32(0x46c).unwrap(), 0);
    assert_eq!(memory.view().read_u8(0x470).unwrap(), 2);
    assert!(keyboard.store_key(KEY));
    assert_eq!(
        execution::run_until(&mut cpu, 0x10d).await,
        ExecutionExit::Interrupted
    );
    assert_eq!(memory.view().read_u8(0x470).unwrap(), 0);
    assert_eq!(memory.view().read_u16(0x502).unwrap(), 0);
    assert_eq!(cpu.cpu_state().sp(), 0x8000);
    assert!(!cpu.cpu_state().interrupts_enabled());
}

macro_rules! test_backends {
    ($name:ident, $check:ident) => {
        #[cfg(not(target_arch = "wasm32"))]
        #[test]
        fn $name() {
            pollster::block_on($check());
        }
        #[cfg(target_arch = "wasm32")]
        #[wasm_bindgen_test::wasm_bindgen_test]
        async fn $name() {
            $check().await;
        }
    };
}

test_backends!(
    timer_irqs_advance_clock_while_bios_waits_for_a_key,
    clock_during_wait
);
test_backends!(
    ready_bios_services_finish_before_irq_delivery,
    ready_services
);
test_backends!(
    abandoning_a_keyboard_wait_preserves_guest_isr_state,
    abandon_during_irq
);
test_backends!(
    bios_irq0_counts_and_consumes_midnight_rollovers,
    bios_timer_rollover
);
