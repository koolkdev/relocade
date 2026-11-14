mod common;

use common::{error, path_service, service, success, SERVICE_PROGRAM};
use relocade_dos::{Dos, FileAccess};

fn dta_timestamp(dos: &mut Dos) -> (u16, u16) {
    let mut words = [0; 4];
    dos.cpu()
        .memory()
        .view()
        .read(0x10080 + 22, &mut words)
        .unwrap();
    (
        u16::from_le_bytes([words[0], words[1]]),
        u16::from_le_bytes([words[2], words[3]]),
    )
}

fn found_timestamp(dos: &mut Dos, path: &[u8]) -> (u16, u16) {
    success(&path_service(dos, path, 0x4e00, 0x10));
    dta_timestamp(dos)
}

fn set_clock_date(dos: &mut Dos, year: u16, month_day: u16) {
    // Keep date assertions away from midnight without relying on elapsed seconds.
    assert_eq!(service(dos, 0x2d00, 0, 0x0c00, 0).registers.eax as u8, 0);
    assert_eq!(
        service(dos, 0x2b00, 0, year, month_day).registers.eax as u8,
        0
    );
}

#[test]
fn directory_and_truncated_file_timestamps_use_the_dos_clock() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
        set_clock_date(&mut dos, 2000, 0x021d);
        success(&path_service(&mut dos, b"DIR", 0x3900, 0));
        let created = found_timestamp(&mut dos, b"DIR");
        assert_eq!(created.1, 0x285d);
        set_clock_date(&mut dos, 2050, 0x0102);
        assert_eq!(found_timestamp(&mut dos, b"DIR/.").1, 0x285d);
        assert_eq!(found_timestamp(&mut dos, b"DIR"), created);
        dos.mount_file("DIR/DATA.BIN", vec![1, 2, 3], FileAccess::ReadWrite)
            .unwrap();
        let initial = found_timestamp(&mut dos, b"DIR/DATA.BIN");
        assert_eq!(initial.1, 0x8c22);
        let reader = success(&path_service(&mut dos, b"DIR/DATA.BIN", 0x3d00, 0));
        error(&service(&mut dos, 0x4000, reader, 0, 0), 5);
        set_clock_date(&mut dos, 2051, 0x0103);
        success(&service(&mut dos, 0x3e00, reader, 0, 0));
        assert_eq!(found_timestamp(&mut dos, b"DIR/DATA.BIN"), initial);
        success(&path_service(&mut dos, b"DIR/DATA.BIN", 0x3c00, 0));
        assert_eq!(found_timestamp(&mut dos, b"DIR/DATA.BIN").1, 0x8e23);
        set_clock_date(&mut dos, 2052, 0x0104);
        common::terminate(&mut dos);
        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
        assert_eq!(found_timestamp(&mut dos, b"DIR/DATA.BIN").1, 0x9024);
    });
}
