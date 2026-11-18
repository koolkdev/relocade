use super::*;
use crate::{default_configuration, session::tests::with_dos};
use relocade_dos::devices::{CharacterDriver, DriverFuture};
use std::{cell::RefCell, rc::Rc};

struct Log(Rc<RefCell<Vec<u8>>>);

impl CharacterDriver for Log {
    fn name(&self) -> [u8; 8] {
        *b"LOG     "
    }

    fn write<'a>(&'a self, bytes: &'a [u8]) -> DriverFuture<'a, usize> {
        Box::pin(async move {
            let accepted = bytes.len().min(2);
            self.0.borrow_mut().extend_from_slice(&bytes[..accepted]);
            Ok(accepted)
        })
    }
}

async fn device_redirection() {
    with_dos(default_configuration(), async |mut dos, capture| {
        let log = Rc::new(RefCell::new(Vec::new()));
        dos.install_driver(Log(log.clone()));
        let (_input, events) = relocade_frontend::keyboard::channel();
        let (saves, _requests) = async_channel::bounded(1);
        let capture = capture.unwrap();
        let oversized = vec![b'X'; capture.remaining_capacity().await.unwrap()];
        let prompt = Console::new(
            dos.native_console(events),
            Some(Printout { capture, saves }),
        );
        let mut command = b"ECHO ".to_vec();
        command.extend_from_slice(&oversized);
        command.extend_from_slice(b">lPt1");
        assert!(prompt.command(&command).await.unwrap_err().contains("full"));
        command.truncate(5 + oversized.len());
        command.extend_from_slice(b">NUL");
        prompt.command(&command).await.unwrap();

        prompt.command(b"echo \xdb>log").await.unwrap();
        assert_eq!(&*log.borrow(), b"\xdb\r\n");
        prompt.command(b"echo \"A > B\" >PrN:").await.unwrap();
        prompt.command(b">\"lPt1\" ECHO C").await.unwrap();
        assert!(prompt.command(b"printout clear >NUL >LPT1").await.is_err());
        assert!(prompt.command(b"printout clear >missing").await.is_err());
        let job = prompt.printer.as_ref().unwrap();
        assert_eq!(job.capture.snapshot().await.unwrap(), b"\"A > B\"\r\nC\r\n");

        prompt.command(b"printout \"clear\" >nul").await.unwrap();
        assert!(job.capture.snapshot().await.unwrap().is_empty());
        prompt.command(b"help>LPT1").await.unwrap();
        assert!(job
            .capture
            .snapshot()
            .await
            .unwrap()
            .starts_with(b"ECHO text"));
    })
    .await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn commands_redirect_through_dos_after_the_whole_line_parses() {
    pollster::block_on(device_redirection());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn commands_redirect_through_dos_after_the_whole_line_parses() {
    device_redirection().await;
}
