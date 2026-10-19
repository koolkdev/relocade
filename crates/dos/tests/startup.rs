mod common;

use wasm86_dos::{LoadError, LoadOptions};
use wasm86_x86::ExecutionExit;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

// DS points to the PSP at both COM and MZ entry.
const PRINT_TAIL: &[u8] = &[
    0x31, 0xc9, // XOR CX,CX
    0x8a, 0x0e, 0x80, 0, // MOV CL,[0080]: command-tail length
    0xbb, 1, 0, // MOV BX,1: stdout
    0xba, 0x81, 0, // MOV DX,0081: command-tail bytes
    0xb4, 0x40, 0xcd, 0x21, // Write CX bytes from DS:DX.
    0xb8, 0, 0x4c, 0xcd, 0x21, // Exit 0.
];

fn mz_image(code: &[u8]) -> Vec<u8> {
    let size = 0x20 + code.len();
    let mut image = vec![0; 0x20];
    image[..2].copy_from_slice(b"MZ");
    for (offset, value) in [
        (0x02, size as u16), // Bytes in the final file page.
        (0x04, 1),           // One file page.
        (0x08, 2),           // Two header paragraphs.
        (0x0a, 0x10),        // Extra paragraphs for the stack.
        (0x0c, 0x10),
        (0x10, 0x100), // SP within the load segment; CS, SS and IP are zero.
    ] {
        image[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    image.extend_from_slice(code);
    image
}

async fn check_command_tails() {
    let mut dos = common::dos().await;
    let maximum = [b'x'; 126];
    for mz in [false, true] {
        let image = if mz {
            mz_image(PRINT_TAIL)
        } else {
            PRINT_TAIL.to_vec()
        };
        for tail in [&b""[..], &b"  \"two words\" \x82"[..], &maximum[..]] {
            let options = LoadOptions { command_tail: tail };
            if mz {
                dos.load_mz(&image, options).unwrap();
            } else {
                dos.load_com(&image, options).unwrap();
            }
            let mut bytes = [0; 128];
            dos.runtime()
                .machine()
                .memory()
                .read(0x10080, &mut bytes)
                .unwrap();
            assert_eq!(bytes[0] as usize, tail.len());
            assert_eq!(&bytes[1..1 + tail.len()], tail);
            assert_eq!(bytes[1 + tail.len()], 13);
            assert_eq!(dos.runtime().run(100).unwrap(), ExecutionExit::HostReturn);
            assert_eq!(dos.exit_status(), Some(Ok(0)));
            assert_eq!(dos.take_output(), tail);
        }

        let cpu = dos.runtime().machine().cpu().snapshot();
        let mut before = [0; 256];
        dos.runtime()
            .machine()
            .memory()
            .read(0x10000, &mut before)
            .unwrap();
        let options = LoadOptions {
            command_tail: &[b'x'; 127],
        };
        let result = if mz {
            dos.load_mz(&image, options)
        } else {
            dos.load_com(&image, options)
        };
        assert_eq!(result, Err(LoadError::CommandTailTooLong));
        assert_eq!(dos.runtime().machine().cpu().snapshot(), cpu);
        let mut after = [0; 256];
        dos.runtime()
            .machine()
            .memory()
            .read(0x10000, &mut after)
            .unwrap();
        assert_eq!(after, before);
        assert_eq!(dos.exit_status(), Some(Ok(0)));
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn com_and_mz_receive_command_tails() {
    pollster::block_on(check_command_tails());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn com_and_mz_receive_command_tails() {
    check_command_tails().await;
}
