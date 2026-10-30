mod support;

use relocade_machine::{Cpu, Device, DispatchEntry, Hook, HookResult, IoBus, Memory, Runner};
use relocade_x86::{
    Compiler, CpuState, Exception, ExecutionExit, ExecutionProfile, HostExit, PhysicalMapping,
    PhysicalMemoryMap, Segment, SegmentRegister, Segments,
};
use std::{
    cell::{Cell, RefCell},
    future::{poll_fn, Future},
    rc::Rc,
    task::{Context, Poll, Waker},
};

const CODE: u32 = 0x10200;
const HANDLER: u32 = 0x300;
const VECTOR: u8 = 0x60;

#[derive(Default)]
struct InterruptDevice {
    request: Option<u8>,
    waker: Option<Waker>,
    acknowledged: usize,
}

impl InterruptDevice {
    fn raise(&mut self) {
        self.request = Some(VECTOR);
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }
}

impl Device for InterruptDevice {
    fn write_port(&mut self, _: u16, _: u8, _: u32) {
        self.raise();
    }

    fn poll_interrupt(&mut self, cx: &mut Context<'_>) -> Poll<u8> {
        match self.request.take() {
            Some(vector) => {
                self.acknowledged += 1;
                Poll::Ready(vector)
            }
            None => {
                self.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

async fn cpu(code: &[u8], device: &Rc<RefCell<InterruptDevice>>, flags: u32) -> Cpu {
    let clock = support::clock::ManualClock::default();
    let runner = Runner::with_clock(clock);
    let io = IoBus::new([runner.connect(device.clone(), [], std::iter::once(0x80..=0x80))]);
    let mut state = CpuState {
        eip: 0x200,
        segments: Segments::real_mode(),
        ..CpuState::default()
    };
    state.segments.cs = SegmentRegister::real_mode(Segment::Cs, 0x1000);
    state.segments.ss = SegmentRegister::real_mode(Segment::Ss, 0x2000);
    state.registers.esp = 0xabcd_8000;
    state.registers.eax = 0x60;
    state.flags.set_eflags(flags);
    let config = support::configuration(
        Compiler::new(ExecutionProfile::Real16).target(),
        state,
        0x40000,
        PhysicalMemoryMap::new([(0..=0x3ffff, PhysicalMapping::Ram { backing_offset: 0 })])
            .unwrap(),
    )
    .unwrap();
    let memory = Memory::new(config.machine).unwrap();
    memory.view().write(CODE, code).unwrap();
    memory
        .view()
        .write(u32::from(VECTOR) * 4, &[0, 3, 0, 0])
        .unwrap();
    memory.view().write(HANDLER, &[0x90, 0xcf]).unwrap();
    Cpu::new(memory, config.cpu, io, ()).await.unwrap()
}

async fn one_instruction(cpu: &mut Cpu, code: &[u8], compiled: bool) {
    if compiled {
        let module = Compiler::new(ExecutionProfile::Real16)
            .compile_block(0x200, code, 1)
            .unwrap();
        cpu.install_module(
            &module.bytes,
            &[DispatchEntry {
                address: CODE,
                export: &module.entry,
            }],
        )
        .await
        .unwrap();
    } else {
        cpu.install_module(
            relocade_interpreter::module(ExecutionProfile::Real16),
            &[DispatchEntry {
                address: CODE,
                export: "step",
            }],
        )
        .await
        .unwrap();
    }
}

async fn check_device_entry() {
    for compiled in [false, true] {
        let device = Rc::new(RefCell::new(InterruptDevice::default()));
        // OUT triggers the device. INC BX must execute only after IRET resumes it.
        let code = [0xe6, 0x80, 0x43, 0xeb, 0xfe];
        let mut cpu = cpu(&code, &device, 0x40b97).await;
        one_instruction(&mut cpu, &code, compiled).await;
        let observed = Rc::new(RefCell::new(None));
        let capture = observed.clone();
        cpu.install_hooks([Hook::new(HANDLER, async move |call| {
            call.with(|call| {
                let mut frame = [0; 6];
                call.memory.read(0x27ffa, &mut frame).unwrap();
                *capture.borrow_mut() = Some((call.cpu.snapshot(), frame));
                call.cpu.set_eip(HANDLER + 1); // Let the guest execute IRET.
                HookResult::Continue
            })
        })])
        .await
        .unwrap();
        assert_eq!(cpu.run(1).await, ExecutionExit::BudgetExhausted);
        assert_eq!(cpu.with(|call| call.cpu.eip()), 0x202);
        assert_eq!(device.borrow().acknowledged, 0);
        assert_eq!(cpu.run(1).await, ExecutionExit::BudgetExhausted);
        let (entry, frame) = observed.borrow().unwrap();
        assert_eq!(entry.instruction_count, 1, "entry retires no instruction");
        assert_eq!(entry.registers.esp, 0xabcd_7ffa);
        assert_eq!(entry.segments.cs.selector, 0);
        assert_eq!(entry.eip, HANDLER);
        assert_eq!(entry.flags.eflags(), 0x897); // IF, TF and AC cleared.
        assert_eq!(frame, [2, 2, 0, 0x10, 0x97, 0x0b]);
        let restored = cpu.with(|call| call.cpu.snapshot());
        assert_eq!(restored.eip, 0x202);
        assert_eq!(restored.segments.cs.selector, 0x1000);
        assert_eq!(restored.registers.esp, 0xabcd_8000);
        assert_eq!(restored.flags.eflags(), 0xb97); // Word IRET leaves AC clear.
        assert_eq!(restored.instruction_count, 2);
        assert_eq!(device.borrow().acknowledged, 1);
        assert_eq!(cpu.run(2).await, ExecutionExit::BudgetExhausted);
        assert_eq!(cpu.with(|call| call.cpu.bx()), 1);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn bus_acknowledges_one_connected_device_per_poll() {
    let runner = Runner::with_clock(support::clock::ManualClock::default());
    let first = Rc::new(RefCell::new(InterruptDevice::default()));
    let second = Rc::new(RefCell::new(InterruptDevice::default()));
    let io = IoBus::new([
        runner.connect(first.clone(), [], []),
        runner.connect(second.clone(), [], []),
    ]);
    first.borrow_mut().request = Some(0x61);
    second.borrow_mut().request = Some(0x62);
    let mut cx = Context::from_waker(Waker::noop());
    assert_eq!(io.poll_interrupt(&mut cx), Poll::Ready(0x61));
    assert_eq!(first.borrow().acknowledged, 1);
    assert_eq!(second.borrow().request, Some(0x62));
    assert_eq!(second.borrow().acknowledged, 0);
    assert_eq!(io.poll_interrupt(&mut cx), Poll::Ready(0x62));
    assert!(io.poll_interrupt(&mut cx).is_pending());
    assert!(first.borrow().waker.is_some());
    assert!(second.borrow().waker.is_some());
}

async fn check_masks_and_zero_budget() {
    let device = Rc::new(RefCell::new(InterruptDevice::default()));
    let mut cpu = cpu(&[0xeb, 0xfe], &device, 0x202).await;
    cpu.install_hooks([Hook::new(HANDLER, async |_| {
        HookResult::Exit(ExecutionExit::Host(HostExit::Return))
    })])
    .await
    .unwrap();
    device.borrow_mut().raise();
    assert_eq!(cpu.run(0).await, ExecutionExit::BudgetExhausted);
    assert_eq!(device.borrow().acknowledged, 0);
    cpu.with(|call| call.cpu.set_eflags(2));
    assert_eq!(cpu.run(5).await, ExecutionExit::BudgetExhausted);
    assert_eq!(device.borrow().request, Some(VECTOR));
    cpu.with(|call| call.cpu.set_eflags(0x202));
    assert_eq!(cpu.run(1).await, ExecutionExit::Host(HostExit::Return));
    assert_eq!(device.borrow().acknowledged, 1);
}

async fn check_protected_steps() {
    for compiled in [false, true] {
        for (instruction, flags, blocks) in [
            (&[0xfb][..], 2, true),                        // STI from IF=0.
            (&[0xfb][..], 0x202, false),                   // STI from IF=1.
            (&[0x8e, 0xd0][..], 0x202, true),              // MOV SS,AX.
            (&[0x17][..], 0x202, true),                    // POP SS.
            (&[0x0f, 0xb2, 0x06, 0, 8][..], 0x202, false), // LSS AX,[0800].
        ] {
            let device = Rc::new(RefCell::new(InterruptDevice::default()));
            let code = [instruction, &[0xe6, 0x80, 0xeb, 0xfe]].concat();
            let mut cpu = cpu(&code, &device, flags).await;
            cpu.memory().view().write(0x28000, &[0x60, 0]).unwrap();
            cpu.memory()
                .view()
                .write(0x800, &[0x61, 0, 0x60, 0])
                .unwrap();
            one_instruction(&mut cpu, &code, compiled).await;
            cpu.install_hooks([Hook::new(HANDLER, async |_| {
                HookResult::Exit(ExecutionExit::Host(HostExit::Return))
            })])
            .await
            .unwrap();
            assert_eq!(cpu.run(1).await, ExecutionExit::BudgetExhausted);
            assert_eq!(
                cpu.with(|call| call.cpu.instruction_count()),
                if blocks { 2 } else { 1 }
            );
            assert_eq!(device.borrow().request, blocks.then_some(VECTOR));
            assert_eq!(device.borrow().acknowledged, 0);
            if !blocks {
                device.borrow_mut().raise();
            }
            assert_eq!(cpu.run(0).await, ExecutionExit::BudgetExhausted);
            assert_eq!(device.borrow().acknowledged, 0);
            assert_eq!(cpu.run(1).await, ExecutionExit::Host(HostExit::Return));
            assert_eq!(device.borrow().acknowledged, 1);
        }

        // Consecutive inhibitors return through the same synchronous step loop.
        let device = Rc::new(RefCell::new(InterruptDevice::default()));
        let code = [0x8e, 0xd0, 0x8e, 0xd0, 0xe6, 0x80, 0xeb, 0xfe];
        let mut cpu = cpu(&code, &device, 0x202).await;
        one_instruction(&mut cpu, &code, compiled).await;
        cpu.install_hooks([Hook::new(HANDLER, async |_| {
            HookResult::Exit(ExecutionExit::Host(HostExit::Return))
        })])
        .await
        .unwrap();
        assert_eq!(cpu.run(1).await, ExecutionExit::BudgetExhausted);
        assert_eq!(cpu.with(|call| call.cpu.instruction_count()), 3);
        assert_eq!(device.borrow().request, Some(VECTOR));
        assert_eq!(device.borrow().acknowledged, 0);
        assert_eq!(cpu.run(1).await, ExecutionExit::Host(HostExit::Return));
        assert_eq!(device.borrow().acknowledged, 1);
    }
}

async fn check_atomic_pending_call(replace_waiter: bool) {
    let device = Rc::new(RefCell::new(InterruptDevice::default()));
    let mut cpu = cpu(&[0x90, 0x90, 0xeb, 0xfe], &device, 0x202).await;
    let ready = Rc::new(Cell::new(false));
    let input = ready.clone();
    cpu.install_hooks([
        Hook::new(CODE, async move |call| {
            poll_fn(|_| {
                if input.get() {
                    Poll::Ready(())
                } else {
                    Poll::Pending
                }
            })
            .await;
            call.with(|context| {
                context.cpu.set_eip(0x202);
            });
            HookResult::Continue
        }),
        Hook::new(HANDLER, async |_| {
            HookResult::Exit(ExecutionExit::Host(HostExit::Return))
        }),
    ])
    .await
    .unwrap();
    let mut cx = Context::from_waker(Waker::noop());
    let mut run = Box::pin(cpu.run(1));
    assert!(run.as_mut().poll(&mut cx).is_pending());
    device.borrow_mut().raise();
    assert!(run.as_mut().poll(&mut cx).is_pending());
    assert_eq!(device.borrow().acknowledged, 0);
    ready.set(true);
    let exit = if replace_waiter {
        drop(run);
        cpu.run(1).await
    } else {
        run.await
    };
    assert_eq!(exit, ExecutionExit::Host(HostExit::Return));
    assert_eq!(device.borrow().acknowledged, 1);
}

async fn check_entry_fault() {
    let device = Rc::new(RefCell::new(InterruptDevice::default()));
    let mut cpu = cpu(&[0xeb, 0xfe], &device, 0x202).await;
    cpu.with(|call| call.cpu.set_sp(3));
    let before = cpu.with(|call| call.cpu.snapshot());
    device.borrow_mut().raise();
    assert_eq!(
        cpu.run(1).await,
        ExecutionExit::Exception(Exception::StackFault { error_code: 0 })
    );
    assert_eq!(cpu.with(|call| call.cpu.snapshot()), before);
    assert_eq!(
        device.borrow().acknowledged,
        1,
        "the controller already supplied the vector"
    );
}

async fn check_protected_fault() {
    for fault in [&[0x0f, 0x0b][..], &[0xf4][..]] {
        let device = Rc::new(RefCell::new(InterruptDevice::default()));
        let mut cpu = cpu(&[&[0xfb], fault, &[0xeb, 0xfe]].concat(), &device, 2).await;
        let expected = if fault == [0xf4] {
            ExecutionExit::UnsupportedInstruction {
                eip: 0x201,
                opcode: 0xf4,
            }
        } else {
            ExecutionExit::Exception(Exception::InvalidOpcode)
        };
        assert_eq!(cpu.run(1).await, expected);
        assert_eq!(cpu.with(|call| call.cpu.instruction_count()), 1);
        assert_eq!(cpu.with(|call| call.cpu.eip()), 0x201);
        assert_eq!(cpu.with(|call| call.cpu.eflags()), 0x202);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn device_interrupts_preserve_frames_and_resume_current_ip() {
    pollster::block_on(check_device_entry());
}
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn masked_requests_and_zero_budget_do_not_acknowledge() {
    pollster::block_on(check_masks_and_zero_budget());
}
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn protected_instructions_step_their_successor_before_returning() {
    pollster::block_on(check_protected_steps());
}
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn interrupt_entry_reports_guest_stack_faults() {
    pollster::block_on(check_entry_fault());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn pending_calls_finish_before_irq_delivery() {
    pollster::block_on(async {
        check_atomic_pending_call(false).await;
        check_atomic_pending_call(true).await;
    });
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_fault_in_the_protected_step_keeps_the_completed_inhibitor() {
    pollster::block_on(check_protected_fault());
}

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn device_interrupts_preserve_frames_and_resume_current_ip() {
    check_device_entry().await;
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn masked_requests_and_zero_budget_do_not_acknowledge() {
    check_masks_and_zero_budget().await;
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn protected_instructions_step_their_successor_before_returning() {
    check_protected_steps().await;
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn interrupt_entry_reports_guest_stack_faults() {
    check_entry_fault().await;
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn pending_calls_finish_before_irq_delivery() {
    check_atomic_pending_call(false).await;
    check_atomic_pending_call(true).await;
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn a_fault_in_the_protected_step_keeps_the_completed_inhibitor() {
    check_protected_fault().await;
}
