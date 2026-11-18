use super::*;
use std::time::Duration;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

async fn with_dos(
    configuration: Configuration,
    check: impl AsyncFnOnce(Dos, Option<Capture>) + Send + 'static,
) {
    let mut machine =
        relocade_machine::Machine::new(relocade_dos::configuration::memory()).unwrap();
    let cpu = machine
        .create_cpu(relocade_dos::configuration::cpu(), None)
        .unwrap();
    cpu.run_until(
        futures_timer::Delay::new(Duration::from_secs(30)),
        async move |runner| {
            let (dos, printer) = start(runner, configuration).await.unwrap();
            check(dos, printer).await;
        },
    )
    .await
    .unwrap()
    .expect("DOS session timed out");
    machine.shutdown().await.unwrap();
}

async fn lpt1_printer() {
    with_dos(crate::default_configuration(), async |mut dos, printer| {
        // The BIOS and the named DOS device must write to the same peripheral.
        let program = [
            0xeb, 6, b'L', b'P', b'T', b'1', 0, 0xdb, // Filename at 102h, data at 107h.
            0x31, 0xd2, 0xb8, b'B', 0, 0xcd, 0x17, // INT 17h, print B on adapter 0.
            0xba, 2, 1, 0xb8, 1, 0x3d, 0xcd, 0x21, // Open LPT1 for writing.
            0x89, 0xc3, 0xba, 7, 1, 0xb9, 1, 0, 0xb4, 0x40, 0xcd, 0x21, // Write one byte.
            0xb8, 0, 0x4c, 0xcd, 0x21, // Exit.
        ];
        dos.launch_com(&program, Default::default()).unwrap();
        assert_eq!(dos.run().await.unwrap(), 0);
        assert_eq!(printer.unwrap().snapshot().await.unwrap(), b"B\xdb");
    })
    .await;

    with_dos(Configuration::default(), async |dos, printer| {
        assert!(printer.is_none());
        assert!(!dos.device(b"LPT1").unwrap().output_ready().await.unwrap());
    })
    .await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn configured_lpt1_captures_bios_and_dos_output() {
    pollster::block_on(lpt1_printer());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn configured_lpt1_captures_bios_and_dos_output() {
    lpt1_printer().await;
}
