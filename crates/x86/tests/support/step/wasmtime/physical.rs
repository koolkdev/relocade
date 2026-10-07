//! Physical routing and byte-backed MMIO devices for execution tests.

use std::{collections::BTreeMap, sync::Arc};
use wasm86_x86::{PhysicalMapping, PhysicalMemoryMap};
use wasmtime::{Caller, Linker, Memory, MemoryType, Store};

use super::{Event, ExecutionEvents, Input};

pub(super) fn register(
    linker: &mut Linker<ExecutionEvents>,
    store: &mut Store<ExecutionEvents>,
    guest: Memory,
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
    let image = PhysicalMemoryMap::new(backing.chain(devices))
        .unwrap()
        .to_bytes();
    let table = Memory::new(&mut *store, MemoryType::new(1, None)).unwrap();
    table.write(&mut *store, 0, &image).unwrap();
    linker
        .define(&*store, "wasm86", "physicalMap", table)
        .unwrap();
    let devices: Arc<BTreeMap<u32, u32>> = Arc::new(input.mmio_pages.iter().copied().collect());
    let observe = input.observe_mmio;
    let read_devices = devices.clone();
    linker
        .func_wrap(
            "wasm86",
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
                    value |= u64::from(guest.data(&caller)[backing]) << (offset * 8);
                }
                apply_update(&mut caller, guest, table);
                value as i64
            },
        )
        .unwrap();
    linker
        .func_wrap(
            "wasm86",
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
                    guest.data_mut(&mut caller)[backing] = byte;
                }
                apply_update(&mut caller, guest, table);
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

fn apply_update(caller: &mut Caller<'_, ExecutionEvents>, guest: Memory, table: Memory) {
    if let Some(update) = caller.data_mut().mmio_updates.next() {
        for (memory, patches) in [(guest, update.guest), (table, update.map)] {
            for (offset, bytes) in patches {
                memory.write(&mut *caller, offset as usize, &bytes).unwrap();
            }
        }
    }
}
