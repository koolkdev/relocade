mod support;

#[path = "guest_time/driver.rs"]
mod driver;

use relocade_machine::{
    Clock, Cpu, Device, DeviceConnection, DispatchEntry, ExecutionWorker, GuestTimeDriver, Instant,
    IoBus, Memory,
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
    let execution = ExecutionWorker::run_until(std::future::pending::<()>(), async |runner| {
        let start = Instant::now();
        let driver = GuestTimeDriver::new(start, NonZeroU64::new(3).unwrap());
        let clock = driver.clock();
        let clone = clock.clone();
        let config = configuration(u64::MAX - 1);
        let memory = Memory::new(config.machine).unwrap();
        memory.view().write(0x100, &[0x90; 10]).unwrap();
        let mut cpu = runner
            .create_cpu(memory, config.cpu, IoBus::default())
            .await
            .unwrap();
        let binding = driver.bind(&cpu, std::task::Waker::noop());
        // Each instruction contributes exactly a third of a second, including
        // across counter wrap. Reading after each step must not lose fractions.
        for nanos in [333_333_333, 666_666_666, 1_000_000_000] {
            cpu.step();
            assert_eq!(clone.now(), start + Duration::from_nanos(nanos));
            assert_eq!(driver.clock().now(), start + Duration::from_nanos(nanos));
        }
        assert_eq!(cpu.cpu_state().instruction_count(), 1);
        cpu.step();
        assert_eq!(
            driver.clock().now(),
            start + Duration::from_nanos(1_333_333_333)
        );
        for nanos in [1_666_666_666, 2_000_000_000] {
            cpu.step();
            assert_eq!(driver.clock().now(), start + Duration::from_nanos(nanos));
        }
        drop(binding);
        cpu.step();
        assert_eq!(clock.now(), start + Duration::from_secs(2));
        let _binding = driver.bind(&cpu, std::task::Waker::noop());
        cpu.step();
        assert_eq!(clock.now(), start + Duration::from_nanos(2_333_333_333));
        // An idle jump preserves the fractional guest time too.
        use std::{
            future::Future,
            pin::pin,
            task::{Context, Waker},
        };
        let mut wait = pin!(clock.sleep_until(start + Duration::from_millis(2500)));
        assert!(wait
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending());
        assert!(driver.advance_to_next_deadline());
        assert!(wait
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_ready());
        cpu.step();
        cpu.step();
        assert_eq!(clock.now(), start + Duration::from_nanos(3_166_666_667));
    })
    .await;
    assert_eq!(execution.unwrap(), Some(()));
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
    observed: Option<Instant>,
}

impl Device for ClockPort {
    fn write_port(&mut self, _port: u16, _bytes: u8, _value: u32) {
        assert!(self.observed.replace(self.clock.now()).is_none());
    }
}

async fn callback_observation() {
    for compiled in [false, true] {
        let start = Instant::now();
        let driver = GuestTimeDriver::new(start, NonZeroU64::new(1000).unwrap());
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
        driver.synchronize(Some(start + Duration::from_millis(4)));
        assert_eq!(cpu.run().await, ExecutionExit::BudgetExhausted);
        let observed = device.borrow().observed.unwrap();
        assert!(observed >= start + Duration::from_millis(1));
        assert!(observed <= start + Duration::from_millis(4));
        assert_eq!(driver.clock().now(), start + Duration::from_millis(4));
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
