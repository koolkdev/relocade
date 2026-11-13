use super::*;
use relocade_machine::IoBus;

mod transfer;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn connected() -> (Rc<RefCell<Dma>>, IoBus) {
    let dma = Rc::new(RefCell::new(Dma::default()));
    let io = IoBus::new(dma.clone(), |io| Dma::configure_io(io, &dma));
    (dma, io)
}

fn write_word(io: &IoBus, port: u16, value: u16) {
    io.write_port(CLEAR_FLIP_FLOP_PORT, 1, 0);
    for byte in value.to_le_bytes() {
        io.write_port(port, 1, u32::from(byte));
    }
}

fn read_word(io: &IoBus, port: u16) -> u16 {
    io.write_port(CLEAR_FLIP_FLOP_PORT, 1, 0);
    u16::from_le_bytes([io.read_port(port, 1) as u8, io.read_port(port, 1) as u8])
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn channels_keep_their_words_and_pc_page_registers() {
    let (_, io) = connected();
    for (channel, page_port) in [0x87, 0x83, 0x81, 0x82].into_iter().enumerate() {
        let port = channel as u16 * 2;
        write_word(&io, port, 0x1234 + channel as u16);
        write_word(&io, port + 1, 0x5678 + channel as u16);
        io.write_port(page_port, 1, 0xa0 + channel as u32);
    }
    for (channel, page_port) in [0x87, 0x83, 0x81, 0x82].into_iter().enumerate() {
        let port = channel as u16 * 2;
        assert_eq!(read_word(&io, port), 0x1234 + channel as u16);
        assert_eq!(read_word(&io, port + 1), 0x5678 + channel as u16);
        assert_eq!(io.read_port(page_port, 1), 0xa0 + channel as u32);
    }
    assert_eq!(
        io.read_port(0x80, 1),
        0xff,
        "POST port is not a page register"
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn reads_and_writes_share_one_byte_flip_flop() {
    let (_, io) = connected();
    write_word(&io, ADDRESS_PORTS[1], 0x1234);
    write_word(&io, COUNT_PORTS[2], 0x5678);
    assert_eq!(io.read_port(ADDRESS_PORTS[1], 1), 0x34);
    io.write_port(COUNT_PORTS[2], 1, 0xab); // The high byte follows that read.
    assert_eq!(read_word(&io, COUNT_PORTS[2]), 0xab78);
    io.write_port(ADDRESS_PORTS[1], 1, 0xcd);
    io.write_port(CLEAR_FLIP_FLOP_PORT, 1, 0xff); // Data is ignored by the strobe.
    assert_eq!(io.read_port(ADDRESS_PORTS[1], 1), 0xcd);
    assert_eq!(io.read_port(ADDRESS_PORTS[1], 2), 0xffff);
    io.write_port(ADDRESS_PORTS[1], 2, 0);
    assert_eq!(io.read_port(ADDRESS_PORTS[1], 1), 0x12);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn mask_controls_and_master_clear_preserve_programmed_channel_values() {
    let (dma, io) = connected();
    assert_eq!(dma.borrow().mask, 0x0f);
    io.write_port(SINGLE_MASK_PORT, 1, 1);
    assert_eq!(dma.borrow().mask, 0x0d);
    io.write_port(SINGLE_MASK_PORT, 1, 5);
    assert_eq!(dma.borrow().mask, 0x0f);
    io.write_port(MASK_PORT, 1, 0xfa);
    assert_eq!(dma.borrow().mask, 0x0a);
    io.write_port(CLEAR_MASK_PORT, 1, 0xff);
    assert_eq!(dma.borrow().mask, 0);

    write_word(&io, ADDRESS_PORTS[1], 0x1234);
    write_word(&io, COUNT_PORTS[1], 0x5678);
    io.write_port(PAGE_PORTS[1], 1, 9);
    io.write_port(MODE_PORT, 1, 0x59);
    io.write_port(COMMAND_PORT, 1, 4);
    io.write_port(REQUEST_PORT, 1, 5);
    assert_eq!(io.read_port(STATUS_PORT, 1), 0x20);
    assert_eq!(
        io.read_port(STATUS_PORT, 1),
        0x20,
        "requests are not cleared by reading"
    );
    io.write_port(REQUEST_PORT, 1, 1);
    assert_eq!(io.read_port(STATUS_PORT, 1), 0);
    io.write_port(REQUEST_PORT, 1, 5);
    io.read_port(ADDRESS_PORTS[1], 1);
    io.write_port(MASTER_CLEAR_PORT, 1, 0xff);
    assert_eq!(io.read_port(ADDRESS_PORTS[1], 1), 0x34);
    assert_eq!(io.read_port(ADDRESS_PORTS[1], 1), 0x12);
    assert_eq!(read_word(&io, COUNT_PORTS[1]), 0x5678);
    assert_eq!(io.read_port(PAGE_PORTS[1], 1), 9);
    assert_eq!(io.read_port(STATUS_PORT, 1), 0);
    assert_eq!(io.read_port(TEMPORARY_PORT, 1), 0);
    assert_eq!(dma.borrow().mask, 0x0f);
    assert_eq!(dma.borrow().command, 0);
    assert_eq!(dma.borrow().channels[1].mode, 0x58);
}
