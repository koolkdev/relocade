//! In-flight target changes and driver lifetime.

use super::*;
use relocade_machine::{MmioRead, MmioWrite, Sleep};
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Waker},
};

struct ReprogrammedTimer {
    clock: Clock,
    deadline: Timestamp,
    wait: Option<Pin<Box<Sleep>>>,
    cancel: bool,
    writes: usize,
}

impl ReprogrammedTimer {
    fn reprogram(&mut self) {
        self.writes += 1;
        if self.cancel {
            self.wait.take();
        } else {
            let mut wait = Box::pin(self.clock.sleep_until(self.deadline));
            assert!(wait
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending());
            self.wait = Some(wait);
        }
    }
}

impl ReprogrammedTimer {
    fn configure_io(io: &mut relocade_machine::DeviceConnection<'_>, device: &Rc<RefCell<Self>>) {
        let callbacks = relocade_machine::DeviceCallbacks::new(device);
        io.bind_mmio(
            0x8000..=0x8fff,
            &MmioRead::new(|_, _, _| u64::MAX),
            &MmioWrite::new(callbacks.write_mmio(|_, device, _, _, _| device.reprogram())),
        );
        io.bind(
            0x80,
            &relocade_machine::PortRead::new(|_, _, _| u32::MAX),
            &relocade_machine::PortWrite::low_byte(
                callbacks.write(|_, device, _| device.reprogram()),
            ),
        );
    }
}

impl Device for ReprogrammedTimer {}

async fn callback_target_changes() {
    for compiled in [false, true] {
        for mmio in [false, true] {
            for (cancel, has_yield_deadline) in [(false, true), (true, true), (true, false)] {
                let driver = GuestTimeDriver::new(NonZeroU64::new(1000).unwrap());
                let clock = driver.clock();
                let mut wait = Box::pin(
                    clock.sleep_until(
                        driver
                            .clock()
                            .add(Timestamp::from_ticks(0), Duration::from_millis(4)),
                    ),
                );
                if cancel {
                    assert!(wait
                        .as_mut()
                        .poll(&mut Context::from_waker(Waker::noop()))
                        .is_pending());
                }
                let device = Rc::new(RefCell::new(ReprogrammedTimer {
                    clock,
                    deadline: driver
                        .clock()
                        .add(Timestamp::from_ticks(0), Duration::from_millis(4)),
                    wait: cancel.then_some(wait),
                    cancel,
                    writes: 0,
                }));
                let io = IoBus::new(device.clone(), |io| {
                    ReprogrammedTimer::configure_io(io, &device);
                });
                let config = support::configuration(
                    Compiler::new(ExecutionProfile::Real16).target(),
                    CpuState {
                        eip: 0x100,
                        ..CpuState::real_mode()
                    },
                    0x10000,
                    PhysicalMemoryMap::new([
                        (0..=0x7fff, PhysicalMapping::Ram { backing_offset: 0 }),
                        (0x8000..=0x8fff, PhysicalMapping::MMIO),
                        (
                            0x9000..=0xffff,
                            PhysicalMapping::Ram {
                                backing_offset: 0x9000,
                            },
                        ),
                    ])
                    .unwrap(),
                )
                .unwrap();
                let memory = Memory::new(config.machine).unwrap();
                // NOP; write once; eight JMP-next checkpoints; finite host exit.
                let mut code = if mmio {
                    vec![0x90, 0xa2, 0x00, 0x80]
                } else {
                    vec![0x90, 0xe6, 0x80]
                };
                for _ in 0..8 {
                    code.extend_from_slice(&[0xeb, 0]);
                }
                memory.view().write(0x100, &code).unwrap();
                let mut cpu = Cpu::new(memory, config.cpu, io).await.unwrap();
                if compiled {
                    let block = Compiler::new(ExecutionProfile::Real16)
                        .compile_block(0x100, &code, 3)
                        .unwrap();
                    cpu.install_module(
                        &block.bytes,
                        &[DispatchEntry {
                            key: 0x100,
                            export: &block.entry,
                        }],
                    )
                    .await
                    .unwrap();
                }
                cpu.install([relocade_machine::Hook::host(0x100 + code.len() as u32, {
                    let exit = cpu.exit_handle();
                    async move || exit.request()
                })])
                .await
                .unwrap();
                let state = cpu.cpu_state();
                let binding = driver.bind(&cpu, Waker::noop());
                let yield_deadline = has_yield_deadline.then(|| {
                    driver
                        .clock()
                        .add(Timestamp::from_ticks(0), Duration::from_millis(10))
                });
                driver.synchronize(yield_deadline);
                assert_eq!(
                    cpu.run().await,
                    if has_yield_deadline {
                        ExecutionExit::BudgetExhausted
                    } else {
                        ExecutionExit::Host(relocade_x86::HostExit::Return)
                    }
                );
                assert_eq!(state.instruction_count(), if cancel { 10 } else { 4 });
                driver.synchronize(yield_deadline);
                assert_eq!(device.borrow().writes, 1);
                if !cancel {
                    assert!(device
                        .borrow_mut()
                        .wait
                        .as_mut()
                        .unwrap()
                        .as_mut()
                        .poll(&mut Context::from_waker(Waker::noop()))
                        .is_ready());
                }
                drop(binding);
                // Releasing the binding clears the target for subsequent work.
                state.set_eip(0x200);
                let mut finish = vec![0x40; 32];
                finish.extend_from_slice(&[0xeb, 0]);
                cpu.memory().view().write(0x200, &finish).unwrap();
                cpu.install([relocade_machine::Hook::host(0x222, {
                    let stop = cpu.exit_handle();
                    async move || stop.request()
                })])
                .await
                .unwrap();
                assert_eq!(
                    cpu.run().await,
                    ExecutionExit::Host(relocade_x86::HostExit::Return)
                );
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn io_and_mmio_wait_changes_retarget_running_guest_execution() {
    pollster::block_on(callback_target_changes());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn io_and_mmio_wait_changes_retarget_running_guest_execution() {
    callback_target_changes().await;
}

async fn cancelled_binding() {
    use relocade_machine::Hook;
    use std::{cell::Cell, future::poll_fn, task::Poll};
    let driver = GuestTimeDriver::new(NonZeroU64::new(1000).unwrap());
    let mut cpu = support::create_cpu(configuration(0)).await.unwrap();
    cpu.memory()
        .view()
        .write(0x100, &[0x40, 0xeb, 0x0d])
        .unwrap();
    cpu.memory()
        .view()
        .write(0x120, &[0x43, 0xeb, 0x0d])
        .unwrap();
    let state = cpu.cpu_state();
    let ready = Rc::new(Cell::new(false));
    let starts = Rc::new(Cell::new(0));
    cpu.install([
        Hook::host(0x110, {
            let (ready, starts, state) = (ready.clone(), starts.clone(), state.clone());
            async move || {
                starts.set(starts.get() + 1);
                poll_fn(|_| {
                    if ready.get() {
                        Poll::Ready(())
                    } else {
                        Poll::Pending
                    }
                })
                .await;
                state.set_eip(0x120);
            }
        }),
        Hook::host(0x130, {
            let exit = cpu.exit_handle();
            async move || exit.request()
        }),
    ])
    .await
    .unwrap();
    let binding = driver.bind(&cpu, Waker::noop());
    driver.synchronize(Some(
        driver
            .clock()
            .add(Timestamp::from_ticks(0), Duration::from_millis(3)),
    ));
    let mut run = Box::pin(cpu.run());
    assert!(run
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
        .is_pending());
    assert_eq!(state.instruction_count(), 2);
    drop(run);
    drop(binding);
    ready.set(true);
    // The saved service resumes once, and the old target at three cannot stop it.
    assert_eq!(
        cpu.run().await,
        ExecutionExit::Host(relocade_x86::HostExit::Return)
    );
    assert_eq!(state.instruction_count(), 4);
    assert_eq!(starts.get(), 1);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn releasing_the_binding_clears_its_target_and_preserves_the_pending_service() {
    pollster::block_on(cancelled_binding());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn releasing_the_binding_clears_its_target_and_preserves_the_pending_service() {
    cancelled_binding().await;
}

async fn expired_yield_deadline() {
    for compiled in [false, true] {
        for cancel in [false, true] {
            let driver = GuestTimeDriver::new(NonZeroU64::new(1000).unwrap());
            let clock = driver.clock();
            let deadline = driver
                .clock()
                .add(Timestamp::from_ticks(0), Duration::from_millis(40));
            let mut wait = Box::pin(clock.sleep_until(deadline));
            if cancel {
                assert!(wait
                    .as_mut()
                    .poll(&mut Context::from_waker(Waker::noop()))
                    .is_pending());
            }
            let device = Rc::new(RefCell::new(ReprogrammedTimer {
                clock,
                deadline,
                wait: cancel.then_some(wait),
                cancel,
                writes: 0,
            }));
            let config = configuration(0);
            let memory = Memory::new(config.machine).unwrap();
            // STI reaches the yield deadline at ten. Its protected OUT observes that
            // retirement before the first permitted checkpoint at eleven.
            let mut code = vec![0x90; 9];
            code.extend_from_slice(&[0xfb, 0xe6, 0x80, 0xeb, 0xfe]);
            memory.view().write(0x100, &code).unwrap();
            let mut cpu = Cpu::new(
                memory,
                config.cpu,
                IoBus::new(device.clone(), |io| {
                    ReprogrammedTimer::configure_io(io, &device);
                }),
            )
            .await
            .unwrap();
            if compiled {
                let block = Compiler::new(ExecutionProfile::Real16)
                    .compile_block(0x100, &code, 12)
                    .unwrap();
                cpu.install_module(
                    &block.bytes,
                    &[DispatchEntry {
                        key: 0x100,
                        export: &block.entry,
                    }],
                )
                .await
                .unwrap();
            }
            let state = cpu.cpu_state();
            let _binding = driver.bind(&cpu, Waker::noop());
            driver.synchronize(Some(
                driver
                    .clock()
                    .add(Timestamp::from_ticks(0), Duration::from_millis(10)),
            ));
            assert_eq!(cpu.run().await, ExecutionExit::BudgetExhausted);
            assert_eq!(state.instruction_count(), 11);
            assert_eq!(device.borrow().writes, 1);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn protected_callback_updates_preserve_the_expired_yield_deadline() {
    pollster::block_on(expired_yield_deadline());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn protected_callback_updates_preserve_the_expired_yield_deadline() {
    expired_yield_deadline().await;
}
