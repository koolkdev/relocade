use super::*;
use std::time::Duration;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

pub(crate) async fn with_dos(
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
        // BIOS adapter 0, LPT1 and the process standard printer must reach one peripheral.
        let program = [
            0xeb, 6, b'L', b'P', b'T', b'1', 0, 0xdb, // Filename at 102h, data at 107h.
            0x31, 0xd2, 0xb8, b'B', 0, 0xcd, 0x17, // INT 17h, print B on adapter 0.
            0xba, 2, 1, 0xb8, 1, 0x3d, 0xcd, 0x21, // Open LPT1 for writing.
            0x89, 0xc3, 0xba, 7, 1, 0xb9, 1, 0, 0xb4, 0x40, 0xcd, 0x21, // Write one byte.
            0xb4, 5, 0xb2, b'P', 0xcd, 0x21, // INT 21h/AH=05h, print P through handle 4.
            0xb8, 0, 0x4c, 0xcd, 0x21, // Exit.
        ];
        dos.launch_com(&program, Default::default()).unwrap();
        assert_eq!(dos.run().await.unwrap(), 0);
        assert_eq!(printer.unwrap().snapshot().await.unwrap(), b"B\xdbP");
    })
    .await;

    with_dos(Configuration::default(), async |dos, printer| {
        assert!(printer.is_none());
        assert!(!dos.device(b"LPT1").unwrap().output_ready().await.unwrap());
        assert!(!dos.device(b"PRN").unwrap().output_ready().await.unwrap());
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

async fn save_captured_output() {
    use crate::printing::Printout;
    use futures_lite::future::zip;
    use std::io;

    with_dos(crate::default_configuration(), async |dos, capture| {
        let (saves, requests) = async_channel::bounded(1);
        let printout = Printout {
            capture: capture.unwrap(),
            saves,
        };
        let lpt = dos.device(b"LPT1").unwrap();
        lpt.write(b"\xdb").await.unwrap();
        let expected = relocade_printer::text::render_pdf(b"\xdb").unwrap();
        for outcome in [Err(io::Error::other("Disk full")), Ok(None), Ok(Some(()))] {
            let failed = outcome.is_err();
            let (result, pdf) = zip(printout.save_pdf(), async {
                let request = requests.recv().await.unwrap();
                request.complete.send(outcome).unwrap();
                request.pdf
            })
            .await;
            if failed {
                assert_eq!(result.unwrap_err(), "Disk full");
            } else {
                result.unwrap();
            }
            assert_eq!(pdf, expected, "each save receives the entire job");
        }
        printout.capture.clear().await.unwrap();
        assert!(printout
            .save_pdf()
            .await
            .unwrap_err()
            .contains("no printer output"));
    })
    .await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn saving_retains_the_job_until_clear() {
    pollster::block_on(save_captured_output());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn saving_retains_the_job_until_clear() {
    save_captured_output().await;
}
