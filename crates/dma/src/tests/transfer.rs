use super::{connected, read_word, write_word};
use crate::*;
use relocade_machine::{IoBus, MachineConfig, Memory};
use relocade_x86::{PhysicalMapping, PhysicalMemoryMap};

fn memory() -> Memory {
    let map = PhysicalMemoryMap::new([(
        0x20000..=0x2ffff,
        PhysicalMapping::Ram { backing_offset: 0 },
    )])
    .unwrap();
    Memory::new(MachineConfig::new(0x10000, map).unwrap()).unwrap()
}

fn program(io: &IoBus, address: u16, count: u16, mode: u8) {
    io.write_port(SINGLE_MASK_PORT, 1, 5);
    write_word(io, ADDRESS_PORTS[1], address);
    write_word(io, COUNT_PORTS[1], count);
    io.write_port(PAGE_PORTS[1], 1, 2);
    io.write_port(MODE_PORT, 1, u32::from(mode | 1));
    io.write_port(SINGLE_MASK_PORT, 1, 1);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn requests_read_live_mapped_memory_and_wrap_without_changing_the_page() {
    let (dma, io) = connected();
    let memory = memory();
    let mut view = memory.view();
    view.write(0x2fffe, &[0x11, 0x22]).unwrap();
    view.write(0x20000, &[0x33, 0x44]).unwrap();
    program(&io, 0xfffe, 3, 0x48);
    dma.borrow_mut().set_request(1, true);

    assert_eq!(dma.borrow_mut().read_memory(1, &view), Some(0x11));
    assert_eq!(read_word(&io, ADDRESS_PORTS[1]), 0xffff);
    assert_eq!(read_word(&io, COUNT_PORTS[1]), 2);
    view.write_u8(0x2ffff, 0x66).unwrap(); // Read when requested, not when programmed.
    let mut bytes = [0xff; 4];
    assert_eq!(dma.borrow_mut().read(1, &view, &mut bytes), 3);
    assert_eq!(bytes, [0x66, 0x33, 0x44, 0xff]);
    assert_eq!(read_word(&io, ADDRESS_PORTS[1]), 2);
    assert_eq!(read_word(&io, COUNT_PORTS[1]), 0xffff);
    assert_eq!(io.read_port(PAGE_PORTS[1], 1), 2);
    assert_eq!(dma.borrow_mut().read_memory(1, &view), None);
    assert_eq!(io.read_port(STATUS_PORT, 1), 0x22);
    assert_eq!(
        io.read_port(STATUS_PORT, 1),
        0x20,
        "only terminal count clears"
    );
    dma.borrow_mut().set_request(1, false);
    assert_eq!(io.read_port(STATUS_PORT, 1), 0);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn blocked_transfers_preserve_the_address_and_count() {
    let (dma, io) = connected();
    let memory = memory();
    let view = memory.view();
    let mut bytes = [0xff; 2];
    program(&io, 0x100, 0, 0x48);
    assert_eq!(dma.borrow_mut().read_memory(1, &view), None, "no request");
    dma.borrow_mut().set_request(1, true);
    io.write_port(SINGLE_MASK_PORT, 1, 5);
    assert_eq!(
        io.read_port(STATUS_PORT, 1),
        0x20,
        "mask does not hide the request"
    );
    assert_eq!(dma.borrow_mut().read(1, &view, &mut bytes), 0);
    io.write_port(SINGLE_MASK_PORT, 1, 1);
    io.write_port(COMMAND_PORT, 1, 4);
    assert_eq!(dma.borrow_mut().read(1, &view, &mut bytes), 0);
    io.write_port(COMMAND_PORT, 1, 1); // Unsupported memory-to-memory operation.
    assert_eq!(dma.borrow_mut().read(1, &view, &mut bytes), 0);
    io.write_port(COMMAND_PORT, 1, 0);
    for mode in [0x45, 0x41, 0x4d, 0x09, 0x89, 0xc9] {
        io.write_port(MODE_PORT, 1, mode); // Wrong direction or unsupported mode.
        assert_eq!(dma.borrow_mut().read(1, &view, &mut bytes), 0);
    }
    assert_eq!(bytes, [0xff; 2]);
    assert_eq!(read_word(&io, ADDRESS_PORTS[1]), 0x100);
    assert_eq!(read_word(&io, COUNT_PORTS[1]), 0);
    io.write_port(MODE_PORT, 1, 0x49);
    assert_eq!(dma.borrow_mut().read_memory(1, &view), Some(0));
    assert_eq!(
        dma.borrow_mut().read_memory(1, &view),
        None,
        "count zero transfers once"
    );
    assert_eq!(io.read_port(STATUS_PORT, 1), 0x22);
    io.write_port(MASTER_CLEAR_PORT, 1, 0);
    assert_eq!(
        io.read_port(STATUS_PORT, 1),
        0x20,
        "reset does not lower external DREQ"
    );
    assert_eq!(
        dma.borrow_mut().read_memory(1, &view),
        None,
        "reset masks all channels"
    );
    dma.borrow_mut().set_request(1, false);
    io.write_port(CLEAR_MASK_PORT, 1, 0);
    assert_eq!(dma.borrow_mut().read_memory(1, &view), None);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn decrement_wraps_within_the_same_page() {
    let (dma, io) = connected();
    let memory = memory();
    let mut view = memory.view();
    view.write_u8(0x20000, 0x11).unwrap();
    view.write_u8(0x2ffff, 0x22).unwrap();
    program(&io, 0, 1, 0x68);
    let mut bytes = [0xff; 3];
    assert_eq!(dma.borrow_mut().read(1, &view, &mut bytes), 2);
    assert_eq!(bytes, [0x11, 0x22, 0xff]);
    assert_eq!(read_word(&io, ADDRESS_PORTS[1]), 0xfffe);
    assert_eq!(io.read_port(PAGE_PORTS[1], 1), 2);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn maximum_count_transfers_a_full_64_kib_buffer() {
    let (dma, io) = connected();
    let memory = memory();
    let view = memory.view();
    program(&io, 0, 0xffff, 0x48);
    assert_eq!(dma.borrow_mut().read(1, &view, &mut []), 0);
    assert_eq!(read_word(&io, COUNT_PORTS[1]), 0xffff);
    let mut bytes = vec![0xff; 65537];
    assert_eq!(dma.borrow_mut().read(1, &view, &mut bytes[..65535]), 65535);
    assert_eq!(io.read_port(STATUS_PORT, 1), 0);
    assert_eq!(read_word(&io, COUNT_PORTS[1]), 0);
    assert_eq!(dma.borrow_mut().read(1, &view, &mut bytes[65535..]), 1);
    assert!(bytes[..65536].iter().all(|&byte| byte == 0));
    assert_eq!(bytes[65536], 0xff);
    assert_eq!(io.read_port(STATUS_PORT, 1), 2);
    assert_eq!(read_word(&io, ADDRESS_PORTS[1]), 0);
    assert_eq!(dma.borrow_mut().read(1, &view, &mut bytes), 0);
}
