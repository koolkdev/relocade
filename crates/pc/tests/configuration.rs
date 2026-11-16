use relocade_clock_test_support::ManualClock;
use relocade_machine::{worker::wake_connection, Machine, Runner};
use relocade_pc::{Configuration, InitializationError, ParallelPort, Pc, SoundCard};
use std::{
    future::Future,
    pin::pin,
    task::{Context, Poll, Waker},
};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn parallel_configuration_reports_invalid_resources() {
    for (base, irq, sound_card, expected) in [
        (
            0,
            5,
            SoundCard::None,
            "parallel-port base 0x0000 must be nonzero and leave room for three registers",
        ),
        (
            0xfffe,
            5,
            SoundCard::None,
            "parallel-port base 0xfffe must be nonzero and leave room for three registers",
        ),
        (
            0x378,
            3,
            SoundCard::None,
            "parallel port does not support IRQ3; choose IRQ5 or IRQ7",
        ),
        (
            0x378,
            16,
            SoundCard::None,
            "parallel port does not support IRQ16; choose IRQ5 or IRQ7",
        ),
        (
            0x378,
            7,
            SoundCard::SoundBlaster2,
            "parallel port and Sound Blaster both use IRQ7; choose IRQ5 for the parallel port",
        ),
    ] {
        let configuration = Configuration {
            sound_card,
            parallel_port: Some(ParallelPort { base, irq }),
            ..Default::default()
        };
        assert_eq!(configuration.validate(), Err(expected.to_owned()));
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn parallel_configuration_accepts_available_resources() {
    for configuration in [
        Configuration::default(),
        Configuration {
            sound_card: SoundCard::SoundBlaster2,
            ..Default::default()
        },
        Configuration {
            parallel_port: Some(ParallelPort::default()),
            ..Default::default()
        },
        Configuration {
            sound_card: SoundCard::SoundBlaster2,
            parallel_port: Some(ParallelPort { base: 0x278, irq: 5 }),
            ..Default::default()
        },
        Configuration {
            parallel_port: Some(ParallelPort {
                base: 0xfffd,
                irq: 5,
            }),
            ..Default::default()
        },
    ] {
        assert_eq!(configuration.validate(), Ok(()));
    }
}

async fn reject_before_initialization() {
    let clock = ManualClock::default();
    let mut machine = Machine::new(relocade_pc::configuration::memory()).unwrap();
    let prepared = machine
        .create_cpu(relocade_pc::configuration::cpu(), None)
        .unwrap();
    let (_host, endpoint) = wake_connection(prepared.interrupt_handle());
    let mut runner = Runner::new(clock.clock(), prepared, endpoint)
        .await
        .unwrap();
    let memory = runner.memory();
    let io = runner.cpu().context().io.clone();
    memory.view().write_u16(0x408, 0xabcd).unwrap();
    let configuration = Configuration {
        sound_card: SoundCard::SoundBlaster2,
        parallel_port: Some(ParallelPort::default()),
        ..Default::default()
    };
    let expected = configuration.validate().unwrap_err();
    // Rejection must complete before connecting devices to an execution session.
    let result = pin!(Pc::with_configuration(runner, configuration))
        .poll(&mut Context::from_waker(Waker::noop()));
    assert!(matches!(
        result,
        Poll::Ready(Err(InitializationError::Validation(message))) if message == expected
    ));
    assert_eq!(memory.view().read_u16(0x408).unwrap(), 0xabcd);
    assert_eq!(io.read_port(0x20, 1), 0xff, "PIC was not installed");
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn pc_rejects_invalid_configuration_before_installing_hardware() {
    pollster::block_on(reject_before_initialization());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn pc_rejects_invalid_configuration_before_installing_hardware() {
    reject_before_initialization().await;
}
