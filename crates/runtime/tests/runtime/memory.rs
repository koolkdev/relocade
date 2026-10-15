use super::{ram, runtime};
use wasm86_machine::MemoryError;
use wasm86_x86::{
    PhysicalMapping::{Mmio, Ram, Rom},
    PhysicalMemoryMap,
};

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn host_transfers_follow_aliases_rom_holes_and_a20() {
    let map = PhysicalMemoryMap::new([
        (
            0..=0xfff,
            Ram {
                backing_offset: 0x3000,
            },
        ),
        (
            0x1000..=0x1fff,
            Ram {
                backing_offset: 0x3000,
            },
        ),
        (
            0x2000..=0x2fff,
            Rom {
                backing_offset: 0x4000,
            },
        ),
        (0x5000..=0x5fff, Mmio),
    ])
    .unwrap();
    let mut machine = runtime()
        .await
        .attach(super::real_machine(map, ()))
        .await
        .unwrap();
    machine.with_memory(|memory| memory.write_backing(0x4000, &[0x56]).unwrap());
    machine.write_memory(0, &[0x12, 0x34]).unwrap();
    machine.write_memory(0x2000, &[0xff]).unwrap();
    machine.write_memory(0x3000, &[0]).unwrap();
    for (address, expected) in [
        (0x1000, 0x12),
        (0x100001, 0x34),
        (0x2000, 0x56),
        (0x3000, 0xff),
        (0x5000, 0xff),
    ] {
        let mut byte = [0];
        machine.read_memory(address, &mut byte).unwrap();
        assert_eq!(byte, [expected]);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn backing_includes_pages_hidden_by_a20_and_grows_before_remapping() {
    let map = PhysicalMemoryMap::new([
        (0..=0xfff, Ram { backing_offset: 0 }),
        (
            0x100000..=0x100fff,
            Ram {
                backing_offset: 0x20000,
            },
        ),
    ])
    .unwrap();
    let mut machine = runtime()
        .await
        .attach(super::real_machine(map, ()))
        .await
        .unwrap();
    machine.with_memory(|memory| {
        memory.write_backing(0x20000, &[0x42]).unwrap();
        memory.set_a20_enabled(true).unwrap();
    });
    let mut byte = [0];
    machine.read_memory(0x100000, &mut byte).unwrap();
    assert_eq!(byte, [0x42]);
    machine.write_memory(0, &[0x35]).unwrap();
    machine.set_memory_map(ram()).unwrap();
    machine
        .with_memory(|memory| memory.set_a20_enabled(true))
        .unwrap();
    machine.read_memory(0, &mut byte).unwrap();
    assert_eq!(byte, [0x35]);
    machine.read_memory(0x10ffef, &mut byte).unwrap();
    assert_eq!(byte, [0]);
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn invalid_host_transfers_and_device_maps_have_no_partial_effects() {
    let map = PhysicalMemoryMap::new([(0x10f000..=0x10ffff, Ram { backing_offset: 0 })]).unwrap();
    let mut machine = runtime()
        .await
        .attach(super::real_machine(map, ()))
        .await
        .unwrap();
    machine
        .with_memory(|memory| memory.set_a20_enabled(true))
        .unwrap();
    machine.write_memory(0x10ffff, &[0x42]).unwrap();
    assert_eq!(
        machine.write_memory(0x10ffff, &[1, 2]),
        Err(MemoryError::AddressRange)
    );
    let mut bytes = [7, 8];
    assert_eq!(
        machine.read_memory(u32::MAX, &mut bytes),
        Err(MemoryError::AddressRange)
    );
    assert_eq!(bytes, [7, 8]);
    let error = machine
        .with_memory(|memory| memory.set_map(ram()))
        .unwrap_err();
    assert_eq!(
        error,
        MemoryError::InsufficientBacking {
            required: 0x110000,
            available: 0x10000
        }
    );
    machine.read_memory(0x10ffff, &mut bytes[..1]).unwrap();
    assert_eq!(bytes[0], 0x42);
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn backing_transfers_validate_the_entire_range() {
    let map = PhysicalMemoryMap::new([(0..=0xfff, Ram { backing_offset: 0 })]).unwrap();
    let mut machine = runtime()
        .await
        .attach(super::real_machine(map, ()))
        .await
        .unwrap();
    machine.with_memory(|memory| {
        assert_eq!(memory.backing_len(), 0x10000);
        memory.write_backing(0xffff, &[0x42]).unwrap();
        assert_eq!(
            memory.write_backing(0xffff, &[1, 2]),
            Err(MemoryError::BackingRange)
        );
        let mut bytes = [7, 8];
        assert_eq!(
            memory.read_backing(0xffff, &mut bytes),
            Err(MemoryError::BackingRange)
        );
        assert_eq!(bytes, [7, 8]);
        memory.read_backing(0xffff, &mut bytes[..1]).unwrap();
        assert_eq!(bytes[0], 0x42);
    });
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn memory_growth_preserves_backing_and_executes_through_the_new_mapping() {
    let map = PhysicalMemoryMap::new([(0..=0xffff, Ram { backing_offset: 0 })]).unwrap();
    let mut machine = runtime()
        .await
        .attach(super::real_machine(map, ()))
        .await
        .unwrap();
    machine.write_memory(0x100, &[0xb8, 7, 0]).unwrap();
    let map = PhysicalMemoryMap::new([(
        0..=0xffff,
        Ram {
            backing_offset: 0x20000,
        },
    )])
    .unwrap();
    machine.set_memory_map(map).unwrap();
    machine.with_memory(|memory| {
        assert_eq!(memory.backing_len(), 0x30000);
        let mut bytes = [0; 3];
        memory.read_backing(0x100, &mut bytes).unwrap();
        assert_eq!(bytes, [0xb8, 7, 0]);
        memory.read_backing(0x20100, &mut bytes).unwrap();
        assert_eq!(bytes, [0; 3]);
    });
    machine.write_memory(0x100, &[0xb8, 42, 0]).unwrap();
    let mut cpu = machine.cpu();
    cpu.eip = 0x100;
    machine.set_cpu(cpu).unwrap();
    assert_eq!(
        machine.run_slice(1).unwrap().stop,
        wasm86_runtime::SliceStop::WorkLimit
    );
    assert_eq!(machine.cpu().registers.eax, 42);
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn an_error_return_does_not_discard_an_earlier_mapping_change() {
    let mut machine = super::machine(&[0xa0, 0x10, 0]).await;
    let mut cpu = machine.cpu();
    cpu.segments.ds = wasm86_x86::StoredSegment::real_mode(wasm86_x86::Segment::Ds, 0xffff);
    machine.set_cpu(cpu).unwrap();
    let oversized = PhysicalMemoryMap::new([(
        0..=0xfff,
        Ram {
            backing_offset: 0x200000,
        },
    )])
    .unwrap();
    let result = machine.with_memory(|memory| {
        memory.write_backing(0x100000, &[0x42]).unwrap();
        memory.set_a20_enabled(true).unwrap();
        memory.set_map(oversized)
    });
    assert!(matches!(
        result,
        Err(MemoryError::InsufficientBacking { .. })
    ));
    assert_eq!(
        machine.run_slice(1).unwrap().stop,
        wasm86_runtime::SliceStop::WorkLimit
    );
    assert_eq!(machine.cpu().registers.eax, 0x42);
}
