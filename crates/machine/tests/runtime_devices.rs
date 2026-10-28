mod support;
use relocade_machine::{yield_now, Clock, Cpu, Device, Hook, HookResult, IoBus, Memory, Runner};
use relocade_x86::{
    Compiler, CpuState, ExecutionExit, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap,
    Segments,
};
use std::{
    cell::{Cell, RefCell},
    future::Future,
    num::NonZeroU64,
    pin::pin,
    rc::Rc,
    task::{Context, Waker},
    time::Duration,
};
use support::clock::ManualClock;

struct Ticker {
    clock: ManualClock,
    ticks: Rc<Cell<u32>>,
    active: Rc<Cell<bool>>,
}

struct Active(Rc<Cell<bool>>);
impl Drop for Active {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

impl Device for Ticker {
    fn read_port(&mut self, _port: u16, _bytes: u8) -> u32 {
        self.ticks.get()
    }

    async fn run(device: Rc<RefCell<Self>>) {
        let (clock, ticks, active) = {
            let device = device.borrow();
            (
                device.clock.clone(),
                device.ticks.clone(),
                device.active.clone(),
            )
        };
        active.set(true);
        let _active = Active(active);
        loop {
            clock.sleep(Duration::from_millis(2)).await;
            ticks.set(ticks.get() + 1);
        }
    }
}

async fn check_devices() {
    let clock = ManualClock::default();
    let mut runner = Runner::with_clock(clock.clone());
    runner.instruction_quantum = NonZeroU64::new(2).unwrap();
    let ticks = Rc::new(Cell::new(0));
    let active = Rc::new(Cell::new(false));
    let device = Rc::new(RefCell::new(Ticker {
        clock: clock.clone(),
        ticks: ticks.clone(),
        active: active.clone(),
    }));
    let io = IoBus::new([runner.connect(device.clone(), [], [0x80..=0x80])]);
    let config = support::configuration(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            eip: 0x200,
            segments: Segments::real_mode(),
            ..CpuState::default()
        },
        0x1000,
        PhysicalMemoryMap::new([(0..=0xfff, PhysicalMapping::Ram { backing_offset: 0 })]).unwrap(),
    )
    .unwrap();
    let memory = Memory::new(config.machine).unwrap();
    let mut cpu = Cpu::new(memory.clone(), config.cpu, io.clone(), clock.clone())
        .await
        .unwrap();
    memory
        .view()
        .write(0x200, &[0xff, 0x06, 0, 3, 0xeb, 0xfa])
        .unwrap();
    assert_eq!(cpu.run(20).await, ExecutionExit::BudgetExhausted);
    let before = memory.view().read_u16(0x300).unwrap();
    cpu.install_hooks([Hook::<ManualClock>::new(0x100, async |call| {
        call.state.sleep(Duration::from_millis(5)).await;
        call.cpu.set_eip(0x200);
        HookResult::Continue
    })])
    .await
    .unwrap();
    cpu.context().cpu.set_eip(0x100);
    let mut cx = Context::from_waker(Waker::noop());
    {
        let mut run = pin!(runner.run(async |runner| runner.run_cpu(&mut cpu).await));
        assert!(run.as_mut().poll(&mut cx).is_pending());
        assert!(active.get());
        clock.advance(Duration::from_millis(2));
        assert!(run.as_mut().poll(&mut cx).is_pending());
        assert_eq!(ticks.get(), 1);
        assert_eq!(memory.view().read_u16(0x300).unwrap(), before);
        assert_eq!(io.read_port(0x80, 1), 1);
        clock.advance(Duration::from_millis(5));
        assert!(run.as_mut().poll(&mut cx).is_pending());
        let resumed = memory.view().read_u16(0x300).unwrap();
        assert!(resumed > before);
        assert_eq!(ticks.get(), 2);
        assert!(run.as_mut().poll(&mut cx).is_pending());
        assert!(memory.view().read_u16(0x300).unwrap() > resumed);
        assert_eq!(ticks.get(), 2);
        clock.advance(Duration::from_millis(7));
        assert!(run.as_mut().poll(&mut cx).is_pending());
        assert_eq!(ticks.get(), 3);
    }
    assert!(!active.get(), "cancellation drops device activities");
    let runner = Runner::with_clock(clock);
    let result = runner
        .run(async |runner| {
            let _connection = runner.connect(device, [], [0x80..=0x80]);
            yield_now().await;
            assert!(active.get());
            42
        })
        .await;
    assert_eq!(result, 42);
    assert!(!active.get(), "main completion drops device activities");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn cpu_and_registered_devices_sleep_independently_and_end_together() {
    pollster::block_on(check_devices());
}
#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn cpu_and_registered_devices_sleep_independently_and_end_together() {
    check_devices().await;
}
