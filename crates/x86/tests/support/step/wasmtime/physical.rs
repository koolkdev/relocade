//! Physical routing and byte-backed MMIO devices for execution tests.

use relocade_test_support::SharedBytes;
use relocade_x86::{PhysicalMapping, PhysicalMemoryMap};
use std::{collections::BTreeMap, sync::Arc};
use wasmtime::{Caller, Linker, Memory, MemoryType, Store};

use super::{Event, ExecutionEvents, Input};

pub(super) fn register(
    linker: &mut Linker<ExecutionEvents>,
    store: &mut Store<ExecutionEvents>,
    guest: SharedBytes,
    input: &Input,
) {
    let backing = input
        .physical_pages
        .iter()
        .map(|&(page, backing_offset, writable)| {
            (
                page * 4096..=page * 4096 + 4095,
                if writable {
                    PhysicalMapping::Ram { backing_offset }
                } else {
                    PhysicalMapping::Rom { backing_offset }
                },
            )
        });
    let devices = input
        .mmio_pages
        .iter()
        .map(|&(page, _)| (page * 4096..=page * 4096 + 4095, PhysicalMapping::Mmio));
    let mut map = PhysicalMemoryMap::new(backing.chain(devices)).unwrap();
    map.set_a20_enabled(input.a20_enabled);
    for page in &input.watched_backing_pages {
        map.set_code_watched(page * 4096, true);
    }
    let image = map.to_bytes();
    let table = Memory::new(&mut *store, MemoryType::new(1, None)).unwrap();
    table.write(&mut *store, 0, &image).unwrap();
    linker
        .define(&*store, "relocade", "physicalMap", table)
        .unwrap();
    let code_guest = guest.clone();
    linker
        .func_wrap(
            "relocade",
            "writeCode",
            move |mut caller: Caller<'_, ExecutionEvents>, backing: i32, bytes: i32, value: i64| {
                let value = &value.to_le_bytes()[..bytes as usize];
                caller.data_mut().events.push(Event::CodeWrite {
                    backing: backing as u32,
                    value: value.to_vec(),
                });
                code_guest.write(backing as u32, value);
            },
        )
        .unwrap();
    let devices: Arc<BTreeMap<u32, u32>> = Arc::new(input.mmio_pages.iter().copied().collect());
    let observe = input.observe_mmio;
    let read_devices = devices.clone();
    let read_guest = guest.clone();
    linker
        .func_wrap(
            "relocade",
            "readMmio",
            move |mut caller: Caller<'_, ExecutionEvents>, address: i32, bytes: i32| {
                assert!((1..=8).contains(&bytes));
                if observe {
                    caller.data_mut().events.push(Event::MmioRead {
                        address: address as u32,
                        bytes: bytes as u32,
                    });
                }
                // Poison ignored upper bits to verify partial-transfer masking.
                let mut value = if bytes == 8 {
                    0
                } else {
                    u64::MAX << (bytes * 8)
                };
                for offset in 0..bytes as u32 {
                    let address = (address as u32).checked_add(offset).unwrap();
                    let backing = device_backing(&read_devices, table.data(&caller), address);
                    value |= u64::from(read_guest.read(backing as u32, 1)[0]) << (offset * 8);
                }
                apply_update(&mut caller, &read_guest, table);
                value as i64
            },
        )
        .unwrap();
    linker
        .func_wrap(
            "relocade",
            "writeMmio",
            move |mut caller: Caller<'_, ExecutionEvents>, address: i32, bytes: i32, value: i64| {
                assert!((1..=8).contains(&bytes));
                assert!(
                    bytes == 8 || (value as u64) >> (bytes * 8) == 0,
                    "narrow writes are zero-extended"
                );
                let value = &value.to_le_bytes()[..bytes as usize];
                if observe {
                    caller.data_mut().events.push(Event::MmioWrite {
                        address: address as u32,
                        value: value.to_vec(),
                    });
                }
                for (offset, &byte) in value.iter().enumerate() {
                    let address = (address as u32).checked_add(offset as u32).unwrap();
                    let backing = device_backing(&devices, table.data(&caller), address);
                    guest.write(backing as u32, &[byte]);
                }
                apply_update(&mut caller, &guest, table);
            },
        )
        .unwrap();
}

fn device_backing(devices: &BTreeMap<u32, u32>, table: &[u8], address: u32) -> usize {
    let page = address >> 12;
    assert!(
        (page as usize) < PhysicalMemoryMap::PAGE_COUNT,
        "callback must stay within MMIO"
    );
    let offset = page as usize * 8;
    assert_eq!(
        u32::from_le_bytes(table[offset..offset + 4].try_into().unwrap()),
        3,
        "callback must stay within MMIO"
    );
    (devices[&page] + (address & 4095)) as usize
}

fn apply_update(caller: &mut Caller<'_, ExecutionEvents>, guest: &SharedBytes, table: Memory) {
    if let Some(update) = caller.data_mut().mmio_updates.next() {
        for (offset, bytes) in update.guest {
            guest.write(offset, &bytes);
        }
        for (offset, bytes) in update.map {
            table.write(&mut *caller, offset as usize, &bytes).unwrap();
        }
    }
}
