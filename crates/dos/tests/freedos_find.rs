mod common;

use wasm86_dos::{FileAccess, LoadOptions};
use wasm86_x86::ExecutionExit;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

const FIND: &[u8] = include_bytes!("fixtures/freedos-find/FIND.COM");

async fn check_find() {
    let mut dos = common::dos().await;
    dos.mount_file(
        "INPUT.TXT",
        b"first line\r\nneedle here\r\nlast line\r\n".to_vec(),
        FileAccess::ReadOnly,
    )
    .unwrap();
    for (tail, expected_output, exit_code) in [
        (
            &b" \"needle\" INPUT.TXT"[..],
            &b"---------------- INPUT.TXT\r\nneedle here\r\n"[..],
            0,
        ),
        (
            &b" \"absent\" INPUT.TXT"[..],
            &b"---------------- INPUT.TXT\r\n"[..],
            1,
        ),
    ] {
        dos.load_com(
            FIND,
            LoadOptions {
                command_tail: tail,
                program_path: b"C:\\FIND.COM",
                ..Default::default()
            },
        )
        .unwrap();
        // Includes the unmodified binary's decompressor and C runtime startup.
        assert_eq!(
            dos.runtime().run(1_000_000).unwrap(),
            ExecutionExit::HostReturn
        );
        assert_eq!(dos.exit_status(), Some(Ok(exit_code)));
        assert_eq!(dos.take_output(), expected_output);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn freedos_find_filters_a_mounted_file() {
    pollster::block_on(check_find());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn freedos_find_filters_a_mounted_file() {
    check_find().await;
}
