use relocade_clock_test_support::origin as clock_origin;
mod support;

use futures_lite::future::poll_once;
use relocade_machine::{
    Clock, Cpu, Device, DeviceConnection, DispatchEntry, ExecutionWorker, Instant,
    InstructionTimeline, IoBus, Memory,
};
use relocade_x86::{
    Compiler, CpuState, ExecutionExit, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap,
    Segments,
};
use std::{
    cell::RefCell,
    future::Future,
    num::NonZeroU64,
    pin::pin,
    rc::Rc,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    task::{Context, Wake, Waker},
    time::Duration,
};

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
        let timeline = InstructionTimeline::new(clock_origin(), NonZeroU64::new(3).unwrap());
        let clock = timeline.clock();
        let clone = clock.clone();
        let mut wait = pin!(clone.sleep_until(clock_origin() + Duration::from_secs(1)));
        assert!(poll_once(wait.as_mut()).await.is_none());
        timeline.advance_to(clock_origin() + Duration::from_millis(250));

        let config = configuration(u64::MAX - 1);
        let memory = Memory::new(config.machine).unwrap();
        memory.view().write(0x100, &[0x90; 6]).unwrap();
        let mut cpu = runner
            .create_cpu(memory, config.cpu, IoBus::default())
            .await
            .unwrap();
        timeline.bind_cpu(cpu.cpu_state());
        assert_eq!(
            timeline.clock().now(),
            clock_origin() + Duration::from_millis(250)
        );
        assert_eq!(
            timeline.next_deadline(),
            Some(clock_origin() + Duration::from_secs(1))
        );

        // Each instruction contributes exactly a third of a second, including
        // across counter wrap. Reading after each step must not lose fractions.
        for nanos in [583_333_333, 916_666_666, 1_250_000_000] {
            cpu.step();
            assert_eq!(clone.now(), clock_origin() + Duration::from_nanos(nanos));
            assert_eq!(
                timeline.clock().now(),
                clock_origin() + Duration::from_nanos(nanos)
            );
        }
        assert_eq!(cpu.cpu_state().instruction_count(), 1);
        // Reading time does not dispatch device activities inside CPU execution.
        assert!(poll_once(wait.as_mut()).await.is_none());
        timeline.notify_due();
        assert!(poll_once(wait.as_mut()).await.is_some());
        assert_eq!(timeline.next_deadline(), None);

        cpu.step();
        assert_eq!(
            timeline.clock().now(),
            clock_origin() + Duration::from_nanos(1_583_333_333)
        );
        timeline.advance_to(clock_origin() + Duration::from_secs(2));
        for nanos in [2_333_333_333, 2_666_666_667] {
            cpu.step();
            assert_eq!(
                timeline.clock().now(),
                clock_origin() + Duration::from_nanos(nanos)
            );
        }
    })
    .await;
    assert_eq!(execution.unwrap(), Some(()));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn worker_clock_tracks_retirement_wrap_and_preserves_fractional_time() {
    pollster::block_on(progress());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn worker_clock_tracks_retirement_wrap_and_preserves_fractional_time() {
    progress().await;
}

#[derive(Default)]
struct Notifications(AtomicUsize);

impl Wake for Notifications {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn deadlines_follow_registration_cancellation_and_idle_time() {
    let timeline = InstructionTimeline::new(clock_origin(), NonZeroU64::new(1000).unwrap());
    let clock = timeline.clock();
    let clone = clock.clone();
    let notifications = Arc::new(Notifications::default());
    let waker = Waker::from(notifications.clone());
    let mut cx = Context::from_waker(&waker);
    let mut later = pin!(clock.sleep_until(clock_origin() + Duration::from_millis(20)));
    assert_eq!(timeline.next_deadline(), None);
    assert!(later.as_mut().poll(&mut cx).is_pending());
    {
        let mut cancelled = pin!(clone.sleep_until(clock_origin() + Duration::from_millis(5)));
        assert!(cancelled.as_mut().poll(&mut cx).is_pending());
        assert_eq!(
            timeline.next_deadline(),
            Some(clock_origin() + Duration::from_millis(5))
        );
    }
    assert_eq!(
        timeline.next_deadline(),
        Some(clock_origin() + Duration::from_millis(20))
    );

    let mut first = pin!(clock.sleep_until(clock_origin() + Duration::from_millis(10)));
    let mut same = pin!(clone.sleep_until(clock_origin() + Duration::from_millis(10)));
    assert!(first.as_mut().poll(&mut cx).is_pending());
    assert!(same.as_mut().poll(&mut cx).is_pending());
    timeline.advance_to(clock_origin() + Duration::from_millis(9));
    assert_eq!(notifications.0.load(Ordering::Relaxed), 0);
    assert!(first.as_mut().poll(&mut cx).is_pending());
    assert_eq!(
        timeline.next_deadline(),
        Some(clock_origin() + Duration::from_millis(10))
    );
    timeline
        .clone()
        .advance_to(clock_origin() + Duration::from_millis(10));
    assert!(notifications.0.load(Ordering::Relaxed) > 0);
    assert!(first.as_mut().poll(&mut cx).is_ready());
    assert!(same.as_mut().poll(&mut cx).is_ready());
    assert!(later.as_mut().poll(&mut cx).is_pending());
    assert_eq!(
        timeline.next_deadline(),
        Some(clock_origin() + Duration::from_millis(20))
    );

    timeline.advance_to(clock_origin() + Duration::from_millis(5));
    assert_eq!(clone.now(), clock_origin() + Duration::from_millis(10));
    let mut overdue = pin!(clock.sleep_until(clock_origin() + Duration::from_millis(5)));
    assert!(overdue.as_mut().poll(&mut cx).is_ready());
    assert_eq!(
        timeline.next_deadline(),
        Some(clock_origin() + Duration::from_millis(20))
    );
    timeline.advance_to(clock_origin() + Duration::from_millis(20));
    assert!(later.as_mut().poll(&mut cx).is_ready());
    assert_eq!(timeline.next_deadline(), None);
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
        let timeline = InstructionTimeline::new(clock_origin(), NonZeroU64::new(1000).unwrap());
        let device = Rc::new(RefCell::new(ClockPort {
            clock: timeline.clock(),
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
        timeline.bind_cpu(cpu.cpu_state());
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
        assert_eq!(cpu.run_with_budget(3).await, ExecutionExit::BudgetExhausted);
        let observed = device.borrow().observed.unwrap();
        assert!(observed >= clock_origin() + Duration::from_millis(1));
        assert!(observed <= clock_origin() + Duration::from_millis(4));
        assert_eq!(
            timeline.clock().now(),
            clock_origin() + Duration::from_millis(4)
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
