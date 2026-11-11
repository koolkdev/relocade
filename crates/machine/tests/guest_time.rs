mod support;

#[path = "guest_time/driver.rs"]
mod driver;

use relocade_machine::{
    Clock, Cpu, Device, DeviceConnection, DispatchEntry, GuestTimeDriver, IoBus, Memory, Timestamp,
};
use relocade_x86::{
    Compiler, CpuState, ExecutionExit, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap,
    Segments,
};
use std::{cell::RefCell, num::NonZeroU64, rc::Rc, time::Duration};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn configuration(count: u64) -> support::Configuration {
    support::configuration(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            eip: 0x100,
            instruction_count: count,
            segments: Segments::real_mode(),
            ..CpuState::default()
        },
        0x10000,
        PhysicalMemoryMap::new([(0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 })]).unwrap(),
    )
    .unwrap()
}

async fn progress() {
    let setup = configuration(u64::MAX - 1);
    let mut machine = relocade_machine::Machine::new(setup.machine).unwrap();
    let prepared = machine.create_cpu(setup.cpu, None).unwrap();
    let execution = prepared.run_until(std::future::pending::<()>(), async |mut runner| {
        let driver = GuestTimeDriver::new(NonZeroU64::new(3).unwrap());
        let clock = driver.clock();
        let clone = clock.clone();
        let memory = runner.memory();
        memory.view().write(0x100, &[0x90; 10]).unwrap();
        let cpu = runner.cpu();
        let start = clock.now();
        let fractional_deadline = clock.add(start, Duration::from_nanos(333_333_334));
        let binding = driver.bind(cpu, std::task::Waker::noop());
        // Each instruction contributes exactly a third of a second, including
        // across counter wrap. Repeated consumption accounts for every ns.
        let mut consumed = start;
        let mut elapsed = Duration::ZERO;
        for nanos in [
            333_333_333,
            666_666_666,
            1_000_000_000,
            1_333_333_333,
            1_666_666_666,
            2_000_000_000,
        ] {
            cpu.step();
            let reading = clone.now();
            elapsed += clock.consume_elapsed(&mut consumed, reading);
            assert_eq!(
                clock.duration_since(reading, start),
                Duration::from_nanos(nanos)
            );
            assert_eq!(elapsed, Duration::from_nanos(nanos));
            assert_eq!(reading >= fractional_deadline, nanos > 333_333_333);
            assert_eq!(driver.clock().now(), reading);
        }
        assert_eq!(cpu.cpu_state().instruction_count(), 4);
        drop(binding);
        cpu.step();
        assert_eq!(clock.now(), clock.add(start, Duration::from_secs(2)));
        let _binding = driver.bind(cpu, std::task::Waker::noop());
        cpu.step();
        assert_eq!(
            clock.duration_since(clock.now(), start),
            Duration::from_nanos(2_333_333_333)
        );
        // Idle advancement reaches the exact deadline; retired fractions then
        // accumulate from that position rather than adding pre-jump fractions.
        use std::{
            future::Future,
            pin::pin,
            task::{Context, Waker},
        };
        let deadline = clock.add(start, Duration::from_millis(2500));
        let mut wait = pin!(clock.sleep_until(deadline));
        let mut cx = Context::from_waker(Waker::noop());
        assert!(wait.as_mut().poll(&mut cx).is_pending());
        assert!(driver.advance_to_next_deadline());
        assert!(wait.as_mut().poll(&mut cx).is_ready());
        assert_eq!(clock.now(), deadline);
        cpu.step();
        cpu.step();
        assert_eq!(
            clock.duration_since(clock.now(), start),
            Duration::from_nanos(3_166_666_666)
        );
    });
    let execution = execution.await;
    assert_eq!(execution.unwrap(), Some(()));
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn guest_time_preserves_progress_across_wrap_idle_and_rebinding() {
    pollster::block_on(progress());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn guest_time_preserves_progress_across_wrap_idle_and_rebinding() {
    progress().await;
}

struct ClockPort {
    clock: Clock,
    observed: Option<Timestamp>,
}

impl Device for ClockPort {
    fn write_port(&mut self, _port: u16, _bytes: u8, _value: u32) {
        assert!(self.observed.replace(self.clock.now()).is_none());
    }
}

async fn callback_observation() {
    for compiled in [false, true] {
        let driver = GuestTimeDriver::new(NonZeroU64::new(1000).unwrap());
        let clock = driver.clock();
        let start = clock.now();
        let device = Rc::new(RefCell::new(ClockPort {
            clock: driver.clock(),
            observed: None,
        }));
        let io = IoBus::new([DeviceConnection {
            device: device.clone(),
            ports: std::iter::once(0x80..=0x80).collect(),
            mmio: vec![],
        }]);
        let config = configuration(99);
        let memory = Memory::new(config.machine).unwrap();
        // NOP; NOP; OUT 80h, AL; JMP to itself.
        let code = [0x90, 0x90, 0xe6, 0x80, 0xeb, 0xfe];
        memory.view().write(0x100, &code).unwrap();
        let mut cpu = Cpu::new(memory, config.cpu, io).await.unwrap();
        let _binding = driver.bind(&cpu, std::task::Waker::noop());
        if compiled {
            let block = Compiler::new(ExecutionProfile::Real16)
                .compile_block(0x101, &code[1..], 3)
                .unwrap();
            cpu.install_module(
                &block.bytes,
                &[DispatchEntry {
                    address: 0x101,
                    export: &block.entry,
                }],
            )
            .await
            .unwrap();
        }
        assert_eq!(cpu.step(), ExecutionExit::StepComplete);
        driver.synchronize(Some(clock.add(start, Duration::from_millis(4))));
        assert_eq!(cpu.run().await, ExecutionExit::BudgetExhausted);
        let observed = device.borrow().observed.unwrap();
        assert!(observed >= clock.add(start, Duration::from_millis(1)));
        assert!(observed <= clock.add(start, Duration::from_millis(4)));
        assert_eq!(
            driver.clock().now(),
            clock.add(start, Duration::from_millis(4))
        );
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn synchronous_device_callbacks_can_read_published_guest_time() {
    pollster::block_on(callback_observation());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn synchronous_device_callbacks_can_read_published_guest_time() {
    callback_observation().await;
}

async fn subnanosecond_progress() {
    let driver = GuestTimeDriver::new(NonZeroU64::new(3_000_000_000).unwrap());
    let clock = driver.clock();
    let start = clock.now();
    let deadline = clock.add(start, Duration::from_nanos(1));
    let mut cpu = support::create_cpu(configuration(0)).await.unwrap();
    cpu.memory().view().write(0x100, &[0x90; 3]).unwrap();
    let _binding = driver.bind(&cpu, std::task::Waker::noop());
    for count in 1..=3 {
        cpu.step();
        assert!(clock.now() > start);
        assert_eq!(clock.now() >= deadline, count == 3);
        assert_eq!(
            clock.duration_since(clock.now(), start),
            Duration::from_nanos(u64::from(count == 3))
        );
    }
    cpu.memory().view().write(0x103, &[0x90; 4]).unwrap();
    cpu.step(); // 1 1/3 ns: a full 1 ns sleep requires three more instructions.
    let sleep_start = clock.now();
    use std::{
        future::Future,
        pin::pin,
        task::{Context, Waker},
    };
    let mut wait = pin!(clock.sleep(Duration::from_nanos(1)));
    let mut cx = Context::from_waker(Waker::noop());
    assert!(wait.as_mut().poll(&mut cx).is_pending());
    assert_eq!(
        driver.next_deadline(),
        Some(clock.add(sleep_start, Duration::from_nanos(1)))
    );
    for count in 1..=3 {
        cpu.step();
        driver.synchronize(None);
        assert_eq!(wait.as_mut().poll(&mut cx).is_ready(), count == 3);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_readings_keep_progress_below_one_nanosecond() {
    pollster::block_on(subnanosecond_progress());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn native_readings_keep_progress_below_one_nanosecond() {
    subnanosecond_progress().await;
}
