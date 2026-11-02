use super::support;
use futures_channel::oneshot;
use relocade_machine::{
    real_mode_interrupt::{interrupt, interrupt_dispatch},
    MemoryView,
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
    Some(__interrupt_call) => { si = __interrupt_call, zf = false },
    None => missing,
})]
// Parameter and pattern names may coincide with the function and context names.
fn position(position: Option<u16>) -> Option<u16> {
    position
}

#[interrupt(returns { status => { ah = status, cf = true } })]
fn missing() -> u8 {
    0x86
}

#[interrupt(args(value = ax), returns { value => { ax = value, cf = false } })]
async fn delayed(value: u16, ready: oneshot::Receiver<()>) -> u16 {
    ready.await.unwrap();
    value + 1
}

interrupt_dispatch! {
    selection(ah) {
        0x12: position(None),
        0x13: nested,
        _: missing,
    }
}
interrupt_dispatch! {
    nested(bl) {
        0x45: position(Some(0x1357)),
        _: missing,
    }
}
interrupt_dispatch! {
    wait_for_ready(ah, ready: oneshot::Receiver<()>) {
        _: delayed(ready),
    }
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
    store_value_entry(&mut call).await;
    assert_eq!(call.memory.read_u16(0x302).unwrap(), 0x2345);
    assert_eq!(cpu.snapshot(), before);
    let mut frame = [0; 6];
    call.memory.read(0x100, &mut frame).unwrap();
    assert_eq!(frame, [0x34, 0x12, 0x78, 0x56, 0x57, 0x0a]);

    inspect_request_entry(&mut call).await;
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
    selection_entry(&mut call).await;
    assert_eq!(cpu.eax(), 0x1234_865a);
    assert_eq!(cpu.esi(), 0x8765_4321);
    cpu.set_ah(0x13);
    selection_entry(&mut call).await;
    assert_eq!(cpu.esi(), 0x8765_1357);
    cpu.set_ah(0xff);
    relocade_machine::real_mode_interrupt::set_carry(&cpu, &mut call.memory, false);
    selection_entry(&mut call).await;
    assert_eq!(cpu.ah(), 0x86);
    assert_eq!(cpu.eflags(), 0x202);
    call.memory.read(0x100, &mut frame).unwrap();
    assert_eq!(frame, [0x34, 0x12, 0x78, 0x56, 0x17, 0x0a]);

    // Async inputs are captured on first poll. Cancellation never applies outputs.
    cpu.set_ax(10);
    let (ready, wait) = oneshot::channel();
    let mut pending = Box::pin(wait_for_ready_entry(&mut call, wait));
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
    let mut cancelled = Box::pin(wait_for_ready_entry(&mut call, wait));
    assert!(cancelled.as_mut().poll(&mut cx).is_pending());
    drop(cancelled);
    assert_eq!(cpu.ax(), 30);
    call.memory.read(0x100, &mut frame).unwrap();
    assert_eq!(frame, [0x34, 0x12, 0x78, 0x56, 0x17, 0x0a]);
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
