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
    fn input_available(&self) -> bool {
        true
    }
    fn try_read_character(&self) -> Result<Option<u8>, DosError> {
        Ok(Some(0))
    }
}

#[test]
fn installing_a_replacement_affects_new_opens_without_rebinding_open_handles() {
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
        assert!(!handle.device.input_available());
    }
    assert_eq!(files.device_information(replacement).unwrap(), 0x80c4);
    let Handle::Device(handle) = files.handle(replacement).unwrap() else {
        panic!("device")
    };
    assert_eq!(handle.device.try_read_character().unwrap(), Some(0));
    assert!(chain.find(b"NUL").unwrap().is_null());
    assert!(!chain.find(b"PRN").unwrap().output_ready());
    assert!(chain.find(b"CONSOLE").is_none());
    let next_process = Files::new(chain);
    assert_eq!(next_process.device_information(1).unwrap(), 0x80c4);
}
