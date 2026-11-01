use super::*;
use crate::files::{FileSystem, Files, Handle};

struct Console;
impl CharacterDriver for Console {
    fn name(&self) -> [u8; 8] {
        *b"CON     "
    }
    fn attributes(&self) -> u16 {
        attributes::CONSOLE
    }
}

struct Replacement;
impl CharacterDriver for Replacement {
    fn name(&self) -> [u8; 8] {
        *b"CON     "
    }
    fn attributes(&self) -> u16 {
        attributes::NULL
    }
    fn input_available(&self) -> DriverFuture<'_, bool> {
        Box::pin(async move { Ok(true) })
    }
    fn peek_character(&self) -> DriverFuture<'_, Option<u8>> {
        Box::pin(async move { Ok(Some(0)) })
    }
}

#[test]
fn installing_a_replacement_affects_new_opens_without_rebinding_open_handles() {
    pollster::block_on(async {
        let chain = DeviceChain::default();
        chain.install(Console);
        chain.install(NullDevice);
        chain.install(Unavailable(*b"AUX     "));
        chain.install(Unavailable(*b"PRN     "));
        let filesystem = FileSystem::default();
        let mut files = Files::new(chain.clone());
        let original = files.open(&filesystem, b"CON:", 2).unwrap();
        let duplicate = files.duplicate(original).unwrap();
        // A replacement with different behavior makes driver identity observable.
        chain.install(Replacement);
        let replacement = files.open(&filesystem, b"con.txt", 2).unwrap();
        for handle in [0, 1, 2, original, duplicate] {
            assert_eq!(files.device_information(handle).unwrap(), 0x80c3);
            let Handle::Device(handle) = files.handle(handle).unwrap() else {
                panic!("device")
            };
            assert!(!handle.device.input_available().await.unwrap());
        }
        assert_eq!(files.device_information(replacement).unwrap(), 0x80c4);
        let Handle::Device(handle) = files.handle(replacement).unwrap() else {
            panic!("device")
        };
        assert_eq!(handle.device.peek_character().await.unwrap(), Some(0));
        assert!(chain.find(b"NUL").unwrap().is_null());
        assert!(!chain.find(b"PRN").unwrap().output_ready().await.unwrap());
        assert!(chain.find(b"CONSOLE").is_none());
        let next_process = Files::new(chain);
        assert_eq!(next_process.device_information(1).unwrap(), 0x80c4);
    });
}

struct EndOfInput;
impl CharacterDriver for EndOfInput {
    fn name(&self) -> [u8; 8] {
        *b"EOF     "
    }
    fn read<'a>(&'a self, bytes: &'a mut [u8]) -> DriverFuture<'a, usize> {
        Box::pin(async move {
            // Bytes outside the returned prefix are unspecified, even on EOF.
            bytes.fill(0xcc);
            Ok(0)
        })
    }
}

#[test]
fn character_read_uses_the_transfer_count_to_recognize_eof() {
    pollster::block_on(async {
        let chain = DeviceChain::default();
        chain.install(EndOfInput);
        assert_eq!(
            chain.find(b"EOF").unwrap().read_character().await.unwrap(),
            0
        );
    });
}
