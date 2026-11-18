use super::*;
use crate::{
    console::Console, default_configuration, printing::Printout, session::tests::with_dos,
};
use futures_lite::future::poll_once;
use relocade_dos::{
    devices::{CharacterDriver, DriverFuture},
    FileAccess,
};

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test;

async fn print_guest_files() {
    with_dos(default_configuration(), async |mut dos, capture| {
        let mut text = vec![b'x'; 511];
        text.extend_from_slice(b"\r\t\xdb\x1aignored");
        dos.mount_file(r"C:\DOCS\REPORT.TXT", text, FileAccess::ReadOnly)
            .unwrap();
        // Change the DOS directory through INT 21h/AH=3Bh before entering the prompt.
        let chdir = [
            0xeb, 5, b'D', b'O', b'C', b'S', 0, // Directory name at 102h.
            0xba, 2, 1, 0xb4, 0x3b, 0xcd, 0x21, 0xb8, 0, 0x4c, 0xcd, 0x21,
        ];
        dos.launch_com(&chdir, Default::default()).unwrap();
        assert_eq!(dos.run().await.unwrap(), 0);
        let mut failures = Vec::new();
        let (_keys, events) = relocade_frontend::keyboard::channel();
        let (saves, requests) = async_channel::bounded(1);
        let mut prompt = Console::new(
            dos.native_console(events),
            Some(Printout {
                capture: capture.unwrap(),
                saves,
            }),
        );
        prompt.command(b"echo second >second.txt").await.unwrap();
        assert!(prompt
            .command(b"print SECOND.TXT missing.txt")
            .await
            .unwrap_err()
            .contains("File not found"));
        assert!(prompt.queue.jobs.is_empty());
        assert!(prompt.command(b"print NUL").await.is_err());
        prompt
            .command(br#"print "report.txt" second.txt"#)
            .await
            .unwrap();
        prompt.command(b"print >state.txt").await.unwrap();
        assert!(prompt.command(b"echo lost >second.txt").await.is_err());
        prompt.queue.drain(&mut failures).await;
        assert!(requests.is_empty());
        let mut expected = vec![b'x'; 511];
        expected.extend_from_slice(b"\r        \xdb\x0csecond\r\n\x0c");
        assert_eq!(
            prompt
                .printer
                .as_ref()
                .unwrap()
                .capture
                .snapshot()
                .await
                .unwrap(),
            expected
        );
        prompt.command(b"print >empty.txt").await.unwrap();
        prompt.command(b"echo new >second.txt").await.unwrap();
        assert!(failures.is_empty(), "{failures:?}");
        drop(prompt);
        assert_eq!(
            dos.read_file(r"DOCS\STATE.TXT").unwrap(),
            b"report.txt\r\nsecond.txt\r\n"
        );
        assert_eq!(
            dos.read_file(r"DOCS\EMPTY.TXT").unwrap(),
            b"Print queue is empty\r\n"
        );
    })
    .await;
}

struct SlowPrinter(async_channel::Sender<u8>);

impl CharacterDriver for SlowPrinter {
    fn name(&self) -> [u8; 8] {
        *b"PRN     "
    }
    fn write<'a>(&'a self, bytes: &'a [u8]) -> DriverFuture<'a, usize> {
        Box::pin(async move {
            self.0
                .send(bytes[0])
                .await
                .map_err(|_| DosError::WriteFault)?;
            Ok(1)
        })
    }
}

async fn cancel_busy_print() {
    with_dos(default_configuration(), async |mut dos, _capture| {
        let (printer, bytes) = async_channel::bounded(1);
        dos.install_driver(SlowPrinter(printer));
        dos.mount_file("A.TXT", b"ABC".to_vec(), FileAccess::ReadOnly)
            .unwrap();
        let mut failures = Vec::new();
        let (_keys, events) = relocade_frontend::keyboard::channel();
        let mut prompt = Console::new(dos.native_console(events), None);
        prompt.command(b"print a.txt").await.unwrap();
        assert!(poll_once(prompt.queue.drain(&mut failures)).await.is_none());
        prompt.command(b"print >queue.txt").await.unwrap();
        assert_eq!(bytes.try_recv().unwrap(), b'A');
        assert!(poll_once(prompt.queue.drain(&mut failures)).await.is_none());
        assert_eq!(bytes.try_recv().unwrap(), b'B');
        prompt.command(b"print /t").await.unwrap();
        prompt.queue.drain(&mut failures).await;
        assert!(bytes.is_empty(), "cancel stops the partially written file");
        assert!(failures.is_empty(), "{failures:?}");
        drop(prompt);
        assert_eq!(dos.read_file("QUEUE.TXT").unwrap(), b"a.txt\r\n");
    })
    .await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn print_queues_dos_text_files_in_order() {
    pollster::block_on(print_guest_files());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test]
async fn print_queues_dos_text_files_in_order() {
    print_guest_files().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn print_can_list_and_cancel_a_busy_installed_printer() {
    pollster::block_on(cancel_busy_print());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test]
async fn print_can_list_and_cancel_a_busy_installed_printer() {
    cancel_busy_print().await;
}
