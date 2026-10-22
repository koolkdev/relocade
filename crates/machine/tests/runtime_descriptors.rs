mod support;
use relocade_machine::PagedMemoryMap;
use relocade_machine::{Machine, RuntimeState};
use relocade_wasm_runtime::{Backend, WasmBackend};
use relocade_x86::{
    Compiler, CpuState, Descriptor, DescriptorTableRegister, DescriptorTables, Exception, Segment,
    SegmentDefaultSize, SegmentDescriptor, SegmentDescriptorKind, SegmentLimit, SegmentProfile,
    SystemSegmentRegister,
};
use support::Configuration;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn data(base: u32) -> SegmentDescriptor {
    SegmentDescriptor::new(
        base,
        SegmentLimit::bytes(0xffff).unwrap(),
        SegmentDescriptorKind::Data {
            writable: true,
            expand_down: false,
        },
        SegmentDefaultSize::Bits32,
    )
}

fn unaccessed(base: u32) -> Descriptor {
    let mut bytes = Descriptor::from(data(base)).to_bytes();
    bytes[5] &= !1;
    Descriptor::from_bytes(bytes)
}

fn config() -> Configuration {
    let mut tables = DescriptorTables::default();
    tables.insert(0x23, data(0x4000));
    tables.insert(0x0f, data(0x5000));
    let mut map = PagedMemoryMap::default();
    map.map(0x4000..=0x4fff, 0, true).unwrap();
    support::with_descriptors(
        support::configuration(
            Compiler::new(SegmentProfile::Flat32).target(),
            CpuState::default(),
            0x1000,
            map,
        )
        .unwrap(),
        tables,
        Some(0x28),
    )
    .unwrap()
}

fn backend() -> Backend<RuntimeState> {
    Backend::with_state(RuntimeState::default()).unwrap()
}

fn check_shared_descriptors() {
    let Configuration {
        cpu,
        machine: config,
    } = config();
    let mut first = backend();
    let space = Machine::new(&mut first, config).unwrap();
    // Attaching a CPU must not restore the original descriptor image.
    space
        .memory()
        .descriptors()
        .write_at(0x20, data(0x6000))
        .unwrap();
    let memory = space.bind_cpu(&mut first, cpu).unwrap();
    let mut second = backend();
    let peer = space.bind_cpu(&mut second, cpu).unwrap();
    memory.with_host_call(&mut first.context(), |call| {
        let cache = call
            .descriptors()
            .resolve_user_segment(Segment::Fs, 0x23)
            .unwrap()
            .complete();
        assert_eq!(cache.base, 0x6000);
        call.cpu().set_segment(Segment::Fs, cache);
        call.cpu()
            .set_gdtr(DescriptorTableRegister::new(0x1000, 0x27));
        let ldtr = SystemSegmentRegister {
            base: 0x18000,
            ..call.cpu().ldtr()
        };
        call.cpu().set_ldtr(ldtr).unwrap();
    });
    space
        .memory()
        .descriptors()
        .write_at(0x20, data(0x7000))
        .unwrap();
    space
        .memory()
        .descriptors()
        .write_at(0x1020, data(0x8000))
        .unwrap();
    space
        .memory()
        .descriptors()
        .write_at(0x18008, data(0x9000))
        .unwrap();
    memory.with_host_call(&mut first.context(), |call| {
        assert_eq!(call.descriptors().read(0x23).unwrap().base(), 0x8000);
        assert_eq!(call.descriptors().read(0x0f).unwrap().base(), 0x9000);
        assert_eq!(call.cpu().segment(Segment::Fs).base, 0x6000);
    });
    peer.with_host_call(&mut second.context(), |call| {
        assert_eq!(call.descriptors().read(0x23).unwrap().base(), 0x7000);
        assert_eq!(call.descriptors().read(0x0f).unwrap().base(), 0x5000);
        assert_eq!(call.cpu().gdtr(), cpu.cpu().gdtr);
        assert_eq!(call.cpu().ldtr(), cpu.cpu().ldtr);
        assert_eq!(call.cpu().segment(Segment::Fs), cpu.cpu().segments.fs);
    });
    drop(first);
    drop(second);
    assert_eq!(
        space.memory().descriptors().read_at(0x20).unwrap().base(),
        0x7000
    );
}

fn check_retained_load() {
    let Configuration {
        cpu,
        machine: config,
    } = config();
    let mut first = backend();
    let space = Machine::new(&mut first, config).unwrap();
    let memory = space.bind_cpu(&mut first, cpu).unwrap();
    let mut second = backend();
    let peer = space.bind_cpu(&mut second, cpu).unwrap();
    space
        .memory()
        .descriptors()
        .write_at(0x1020, unaccessed(0x9000))
        .unwrap();
    peer.with_host_call(&mut second.context(), |call| {
        call.cpu()
            .set_gdtr(DescriptorTableRegister::new(0x1000, 0x27));
    });
    for complete in [false, true] {
        space
            .memory()
            .descriptors()
            .write_at(0x20, unaccessed(0x6000))
            .unwrap();
        memory.with_host_call(&mut first.context(), |call| {
            let load = call
                .descriptors()
                .resolve_user_segment(Segment::Fs, 0x23)
                .unwrap();
            assert!(load.needs_completion());
            assert_eq!(load.cache().base, 0x6000);
            // Another owner can write unrelated memory while this load is retained.
            space.memory().view().write_u8(0x4000, 1).unwrap();
            peer.with_host_call(&mut second.context(), |peer| {
                assert_eq!(peer.descriptors().read(0x23).unwrap().base(), 0x9000);
            });
            if complete {
                assert_eq!(load.complete().base, 0x6000);
            }
        });
        assert_eq!(
            space
                .memory()
                .descriptors()
                .read_at(0x20)
                .unwrap()
                .to_bytes()[5]
                & 1,
            u8::from(complete)
        );
        // Completion belongs to the original CPU's table, not the peer's GDTR.
        assert_eq!(
            space.memory().descriptors().read_at(0x1020).unwrap(),
            unaccessed(0x9000)
        );
        space.memory().view().write_u8(0x4000, 2).unwrap();
    }
    memory.with_host_call(&mut first.context(), |call| {
        assert_eq!(
            call.descriptors()
                .resolve_user_segment(Segment::Fs, 0x30)
                .err(),
            Some(Exception::GeneralProtection { error_code: 0x30 })
        );
    });
    space
        .memory()
        .descriptors()
        .write_at(0x20, data(0x7000))
        .unwrap();
    space.memory().view().unmap_pages(0x4000..=0x4fff).unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn descriptor_bytes_are_shared_while_registers_and_caches_are_private() {
    check_shared_descriptors();
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
fn descriptor_bytes_are_shared_while_registers_and_caches_are_private() {
    check_shared_descriptors();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn segment_completion_updates_the_resolved_descriptor() {
    check_retained_load();
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
fn segment_completion_updates_the_resolved_descriptor() {
    check_retained_load();
}
