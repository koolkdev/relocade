//! Guest controller writes used by BIOS and DOS mouse integration fixtures.
use relocade_machine::IoBus;

pub async fn wait_status(io: &IoBus, mask: u32, value: u32) {
    for _ in 0..64 {
        if io.read_port(0x64, 1) & mask == value {
            return;
        }
        relocade_machine::yield_now().await;
    }
    panic!("controller did not reach status {mask:#x}={value:#x}");
}

pub async fn write(io: &IoBus, port: u16, byte: u8) {
    wait_status(io, 2, 0).await;
    io.write_port(port, 1, u32::from(byte));
    wait_status(io, 2, 0).await;
}

pub async fn inject(io: &IoBus, byte: u8) {
    write(io, 0x64, 0xd3).await;
    write(io, 0x60, byte).await;
    wait_status(io, 0x21, 0x21).await;
}
