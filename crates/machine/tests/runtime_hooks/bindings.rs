use super::support;
#[path = "bindings/handlers.rs"]
mod handlers;
#[path = "bindings/pointers.rs"]
mod pointers;
use futures_channel::oneshot;
use relocade_machine::{
    real_mode_interrupt::{interrupt, interrupt_dispatch, interrupt_handler},
    CpuContext, MemoryView,
};
use relocade_x86::{
    Compiler, CpuState, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap, Segments,
};
use std::{future::Future, task::Context};

struct Request {
    page: u8,
    offset: u16,
}

#[interrupt(
    context(memory),
    args(request = Request { page: bh, offset: si }, position = (cx, dx), count = edi),
    returns { byte => { al = byte, (cx, dx) = 0x9876_5432u32, edi = 0x1234_5678u32, cf = false } }
)]
fn inspect_request(memory: &mut MemoryView<'_>, request: Request, position: u32, count: u32) -> u8 {
    assert_eq!(
        (request.page, position, request.offset, count),
        (0x23, 0x1234_5678, 0x4321, 0xfedc_ba98)
    );
    memory.write_u8(0x300, request.page).unwrap();
    0x5a
}

#[interrupt(context(memory), args(value = bx))]
fn store_value(memory: &mut MemoryView<'_>, value: u16) {
    memory.write_u16(0x302, value).unwrap();
}

#[interrupt(returns {
    Ok(__interrupt_call) => { si = __interrupt_call, zf = false },
    Err(status) => missing(status),
})]
// Parameter and pattern names may coincide with the function and context names.
fn position(position: Result<u16, u8>) -> Result<u16, u8> {
    position
}

#[interrupt(returns { status => { ah = status, cf = true } })]
fn missing(status: u8) -> u8 {
    status
}

#[interrupt(args(value = ax), returns { value => { ax = value, cf = false } })]
async fn delayed(value: u16, ready: oneshot::Receiver<()>) -> u16 {
    ready.await.unwrap();
    value + 1
}

struct Adjustment(u16);

impl Adjustment {
    #[interrupt(args(value = ax), returns { value => { ax = value } })]
    fn add(&self, value: u16, amount: u16) -> u16 {
        value + self.0 + amount
    }
}

#[interrupt(context(call), args(function = ah))]
async fn selection(call: &mut CpuContext<'_>, function: u8) {
    interrupt_dispatch!(
        call,
        match function {
            0x12 => position(Err(0x86)),
            function @ 0x13..=0x14 if function == 0x13 => nested,
            0x13 | 0x14 => position(Ok(0x2468)),
            _ => missing(0x86),
        }
    );
}
#[interrupt(context(call), args(function = bl))]
async fn nested(call: &mut CpuContext<'_>, function: u8) {
    interrupt_dispatch!(
        call,
        match function {
            0x45 => position(Ok(0x1357)),
            _ => missing(0x86),
        }
    );
}
#[interrupt(context(call), args(function = ah))]
async fn wait_for_ready(call: &mut CpuContext<'_>, ready: oneshot::Receiver<()>, function: u8) {
    interrupt_dispatch!(
        call,
        match function {
            _ => delayed(ready),
        }
    );
}

async fn check_bindings() {
    let config = support::configuration(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            segments: Segments::real_mode(),
            ..CpuState::default()
        },
        0x10000,
        PhysicalMemoryMap::new([(0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 })]).unwrap(),
    )
    .unwrap();
    let mut runtime = support::create_cpu(config).await.unwrap();
    let memory = runtime.memory();
    let mut call = runtime.context();
    let cpu = call.cpu.clone();
    cpu.set_eax(0x1234_abcd);
    cpu.set_ebx(0x9876_2345);
    cpu.set_ecx(0xabcd_1234);
    cpu.set_edx(0xefab_5678);
    cpu.set_esi(0x8765_4321);
    cpu.set_edi(0xfedc_ba98);
    cpu.set_sp(0x100);
    cpu.set_eflags(0x202); // Live flags deliberately differ from the saved frame.
    call.memory
        .write(0x100, &[0x34, 0x12, 0x78, 0x56, 0x57, 0x0a])
        .unwrap();
    let before = cpu.snapshot();
    interrupt_handler!(store_value)(&mut call).await;
    assert_eq!(call.memory.read_u16(0x302).unwrap(), 0x2345);
    assert_eq!(cpu.snapshot(), before);
    let mut frame = [0; 6];
    call.memory.read(0x100, &mut frame).unwrap();
    assert_eq!(frame, [0x34, 0x12, 0x78, 0x56, 0x57, 0x0a]);

    interrupt_handler!(inspect_request)(&mut call).await;
    assert_eq!(call.memory.read_u8(0x300).unwrap(), 0x23);
    assert_eq!(cpu.eax(), 0x1234_ab5a);
    assert_eq!(cpu.ebx(), 0x9876_2345);
    assert_eq!(cpu.ecx(), 0xabcd_9876);
    assert_eq!(cpu.edx(), 0xefab_5432);
    assert_eq!(cpu.esi(), 0x8765_4321);
    assert_eq!(cpu.edi(), 0x1234_5678);
    assert_eq!(cpu.eflags(), 0x202);
    call.memory.read(0x100, &mut frame).unwrap();
    assert_eq!(frame, [0x34, 0x12, 0x78, 0x56, 0x56, 0x0a]);

    cpu.set_ah(0x12);
    interrupt_handler!(selection)(&mut call).await;
    assert_eq!(cpu.eax(), 0x1234_865a);
    assert_eq!(cpu.esi(), 0x8765_4321);
    cpu.set_ah(0x13);
    interrupt_handler!(selection)(&mut call).await;
    assert_eq!(cpu.esi(), 0x8765_1357);
    cpu.set_ah(0x14);
    interrupt_handler!(selection)(&mut call).await;
    assert_eq!(cpu.esi(), 0x8765_2468);
    cpu.set_ah(0xff);
    relocade_machine::real_mode_interrupt::set_carry(&cpu, &mut call.memory, false);
    interrupt_handler!(selection)(&mut call).await;
    assert_eq!(cpu.ah(), 0x86);
    assert_eq!(cpu.eflags(), 0x202);
    call.memory.read(0x100, &mut frame).unwrap();
    assert_eq!(frame, [0x34, 0x12, 0x78, 0x56, 0x17, 0x0a]);

    // Async inputs are captured on first poll. Cancellation never applies outputs.
    cpu.set_ax(10);
    let (ready, wait) = oneshot::channel();
    let mut pending = Box::pin(interrupt_handler!(wait_for_ready)(&mut call, wait));
    let mut cx = Context::from_waker(std::task::Waker::noop());
    assert!(pending.as_mut().poll(&mut cx).is_pending());
    assert_eq!(cpu.ax(), 10);
    memory.view().read(0x100, &mut frame).unwrap();
    assert_eq!(frame, [0x34, 0x12, 0x78, 0x56, 0x17, 0x0a]);
    cpu.set_ax(20);
    ready.send(()).unwrap();
    assert!(pending.as_mut().poll(&mut cx).is_ready());
    drop(pending);
    assert_eq!(cpu.ax(), 11);
    call.memory.read(0x100, &mut frame).unwrap();
    assert_eq!(frame, [0x34, 0x12, 0x78, 0x56, 0x16, 0x0a]);

    cpu.set_ax(30);
    relocade_machine::real_mode_interrupt::set_carry(&cpu, &mut call.memory, true);
    let (_ready, wait) = oneshot::channel();
    let mut cancelled = Box::pin(interrupt_handler!(wait_for_ready)(&mut call, wait));
    assert!(cancelled.as_mut().poll(&mut cx).is_pending());
    drop(cancelled);
    assert_eq!(cpu.ax(), 30);
    call.memory.read(0x100, &mut frame).unwrap();
    assert_eq!(frame, [0x34, 0x12, 0x78, 0x56, 0x17, 0x0a]);

    // The match evaluates its input once and forwards bound values only from
    // the selected arm. Exhaustive enum patterns need no wildcard fallback.
    let adjustment = Adjustment(0x10);
    let call = &mut call;
    for (choice, expected, arguments) in [(Some(3), 0x18, 1), (Some(8), 0x1d, 1), (None, 0x15, 0)] {
        cpu.set_ax(5);
        let mut evaluations = 0;
        let mut forwarded = 0;
        let mut read_choice = || {
            evaluations += 1;
            choice
        };
        interrupt_dispatch!(
            call,
            match read_choice() {
                Some(value @ 0..=7) if value < 4 => adjustment.add({
                    forwarded += 1;
                    value
                }),
                Some(value) => adjustment.add({
                    forwarded += 1;
                    value
                }),
                None => adjustment.add(0),
            }
        );
        assert_eq!(cpu.ax(), expected);
        assert_eq!(evaluations, 1);
        assert_eq!(forwarded, arguments);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn register_bindings() {
    pollster::block_on(check_bindings());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn register_bindings() {
    check_bindings().await;
}
