use super::support;
use futures_channel::oneshot;
use relocade_machine::{
    real_mode_interrupt::{interrupt, interrupt_handler, FarPointer},
    MemoryView,
};
use relocade_x86::{
    Compiler, CpuState, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap, Segment,
    SegmentRegister,
};
use std::{future::Future, task::Context};

#[interrupt(context(memory), args(source = ptr(es, si)), returns {
    address => { ptr(es, bp) = address },
})]
async fn locate_font(
    memory: &MemoryView<'_>,
    source: FarPointer,
    ready: oneshot::Receiver<()>,
) -> FarPointer {
    ready.await.unwrap();
    assert_eq!(source.packed(), 0x1234_0050);
    let mut bytes = [0; 2];
    memory.read(source.address(), &mut bytes).unwrap();
    assert_eq!(bytes, [0xa5, 0x3c]);
    FarPointer::from_packed(0x4321_0070)
}

#[interrupt(args(selector = es), returns { selector => { ds = selector } })]
fn select_data_segment(selector: u16) -> u16 {
    selector
}

async fn check_pointer_bindings() {
    let config = support::configuration(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState::real_mode(),
        0x20000,
        PhysicalMemoryMap::new([(0..=0x1ffff, PhysicalMapping::Ram { backing_offset: 0 })])
            .unwrap(),
    )
    .unwrap();
    let mut runtime = support::create_cpu(config).await.unwrap();
    let mut call = runtime.context();
    let cpu = call.cpu.clone();
    cpu.set_segment(Segment::Es, SegmentRegister::real_mode(Segment::Es, 0x1234));
    interrupt_handler!(select_data_segment)(&mut call).await;
    assert_eq!(
        cpu.segment(Segment::Ds),
        SegmentRegister::real_mode(Segment::Ds, 0x1234)
    );
    cpu.set_esi(0x9876_0050);
    cpu.set_ebp(0xabcd_2222);
    call.memory.write(0x12390, &[0xa5, 0x3c]).unwrap();

    let before = cpu.snapshot();
    let (ready, wait) = oneshot::channel();
    let mut pending = Box::pin(interrupt_handler!(locate_font)(&mut call, wait));
    let mut cx = Context::from_waker(std::task::Waker::noop());
    assert!(pending.as_mut().poll(&mut cx).is_pending());
    assert_eq!(cpu.snapshot(), before);

    // Input pointers retain both words from the first poll.
    cpu.set_segment(Segment::Es, SegmentRegister::real_mode(Segment::Es, 0x3333));
    cpu.set_si(0x1111);
    let mut expected = cpu.snapshot();
    ready.send(()).unwrap();
    assert!(pending.as_mut().poll(&mut cx).is_ready());
    drop(pending);
    expected.segments.es = SegmentRegister::real_mode(Segment::Es, 0x4321);
    expected.registers.ebp = 0xabcd_0070;
    assert_eq!(cpu.snapshot(), expected);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn guest_pointer_bindings() {
    pollster::block_on(check_pointer_bindings());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn guest_pointer_bindings() {
    check_pointer_bindings().await;
}
