use crate::test_runner::compilation;

use super::*;
use crate::files::{FileSystem, Files, Handle};
use relocade_bios::KeyMode;
use relocade_machine::{MachineConfig, Memory};
use relocade_x86::{PhysicalMapping, PhysicalMemoryMap};

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
        let map = PhysicalMemoryMap::new([(0..=0xfff, PhysicalMapping::Ram { backing_offset: 0 })])
            .unwrap();
        let memory = Memory::new(MachineConfig::new(4096, map).unwrap()).unwrap();
        let stdio = crate::stdio::Stdio::new(relocade_bios::Keyboard::new(memory));
        let mut files = Files::new(chain.clone(), &stdio);
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
        let next_process = Files::new(chain, &stdio);
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

#[test]
fn console_drivers_keep_their_own_extended_key_bytes_when_replaced_or_flushed() {
    use relocade_bios::KeyStroke;
    use relocade_machine::timer_service;

    pollster::block_on(compilation::run(async |compiler| {
        let (clock, _timers) = timer_service();
        let (runner, _host, machine) = crate::test_runner::runner(clock.into(), compiler).await;
        let mut dos = runner
            .run(std::future::pending::<()>(), async |runner| {
                crate::Dos::new(runner).await.unwrap()
            })
            .await
            .unwrap();
        machine.shutdown().await.unwrap();
        dos.cpu().interrupt_handle().take();
        let keyboard = dos.keyboard();
        let original = dos.state.devices().console();
        assert!(keyboard.store_key(KeyStroke {
            character: 0,
            scan_code: 0x3b, // F1.
        }));
        assert_eq!(original.read_character().await.unwrap(), 0);
        assert_eq!(keyboard.peek_key(KeyMode::Enhanced), None);

        let memory = dos.cpu().memory().with_io(dos.cpu().context().io.clone());
        dos.state.devices().install(BiosConsole::new(
            memory,
            ConsoleInput::new(keyboard.clone()),
        ));
        let replacement = dos.state.devices().console();
        assert_eq!(replacement.peek_character().await.unwrap(), None);
        assert!(!replacement.input_available().await.unwrap());
        assert_eq!(original.peek_character().await.unwrap(), Some(0x3b));
        assert!(original.input_available().await.unwrap());

        assert!(keyboard.store_key(KeyStroke {
            character: 0,
            scan_code: 0x48, // Up arrow.
        }));
        assert_eq!(replacement.read_character().await.unwrap(), 0);
        assert_eq!(replacement.peek_character().await.unwrap(), Some(0x48));
        assert!(keyboard.store_key(KeyStroke {
            character: b'Z',
            scan_code: 0x2c,
        }));
        replacement.flush_input().await.unwrap();
        assert_eq!(keyboard.peek_key(KeyMode::Enhanced), None);
        assert_eq!(replacement.peek_character().await.unwrap(), None);
        assert_eq!(original.read_character().await.unwrap(), 0x3b);
        assert_eq!(original.peek_character().await.unwrap(), None);
    }));
}
