use super::{ram, runtime};
use wasm86_runtime::MemoryError;
use wasm86_x86::{
    PhysicalMapping::{Mmio, Ram, Rom},
    PhysicalMemoryMap,
};

#[test]
fn host_transfers_follow_aliases_rom_holes_and_a20() {
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
    let mut machine = runtime().machine(map, ()).unwrap();
    machine.with_memory(|memory| memory.backing_mut()[0x4000] = 0x56);
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

#[test]
fn backing_includes_pages_hidden_by_a20_and_grows_before_remapping() {
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
    let mut machine = runtime().machine(map, ()).unwrap();
    machine.with_memory(|memory| {
        memory.backing_mut()[0x20000] = 0x42;
        memory.set_a20_enabled(true);
    });
    let mut byte = [0];
    machine.read_memory(0x100000, &mut byte).unwrap();
    assert_eq!(byte, [0x42]);
    machine.write_memory(0, &[0x35]).unwrap();
    machine.set_memory_map(ram()).unwrap();
    machine.with_memory(|memory| memory.set_a20_enabled(true));
    machine.read_memory(0, &mut byte).unwrap();
    assert_eq!(byte, [0x35]);
    machine.read_memory(0x10ffef, &mut byte).unwrap();
    assert_eq!(byte, [0]);
}

#[test]
fn invalid_host_transfers_and_device_maps_have_no_partial_effects() {
    let map = PhysicalMemoryMap::new([(0x10f000..=0x10ffff, Ram { backing_offset: 0 })]).unwrap();
    let mut machine = runtime().machine(map, ()).unwrap();
    machine.with_memory(|memory| memory.set_a20_enabled(true));
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
