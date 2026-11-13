use relocade_clock_test_support::ManualClock;
use relocade_machine::{worker::wake_connection, Device, Hook, PortRead, PortWrite, Runner};
use relocade_pc::{Configuration, Pc, SoundCard};
use relocade_sound_blaster::{READ_DATA_PORT, READ_STATUS_PORT, RESET_PORT, WRITE_DATA_PORT};
use relocade_x86::{ExecutionExit, HostExit};
use std::{cell::RefCell, rc::Rc, time::Duration};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

struct ClockStep;
impl Device for ClockStep {}

fn out(code: &mut Vec<u8>, port: u16, value: u8) {
    code.extend_from_slice(&[0xba, port as u8, (port >> 8) as u8, 0xb0, value, 0xee]);
}

fn read_to(code: &mut Vec<u8>, port: u16, address: u16) {
    code.extend_from_slice(&[
        0xba,
        port as u8,
        (port >> 8) as u8,
        0xec,
        0xa2,
        address as u8,
        (address >> 8) as u8,
    ]);
}

fn dma(code: &mut Vec<u8>, address: u16, count: u8) {
    use relocade_dma::*;
    for (port, value) in [
        (SINGLE_MASK_PORT, 5),
        (CLEAR_FLIP_FLOP_PORT, 0),
        (ADDRESS_PORTS[1], address as u8),
        (ADDRESS_PORTS[1], (address >> 8) as u8),
        (COUNT_PORTS[1], count),
        (COUNT_PORTS[1], 0),
        (PAGE_PORTS[1], 0),
        (MODE_PORT, 0x49),
        (SINGLE_MASK_PORT, 1),
    ] {
        out(code, port, value);
    }
    for value in [0x14, count, 0] {
        out(code, WRITE_DATA_PORT, value);
    }
}

fn wait_irq(code: &mut Vec<u8>, count: u8) {
    // CMP BYTE [0700],count; JNE back to CMP.
    code.extend_from_slice(&[0x80, 0x3e, 0, 7, count, 0x75, 0xf9]);
}

async fn exercise(capture: bool) {
    let clock = ManualClock::new();
    let mut machine = relocade_machine::Machine::new(relocade_pc::configuration::memory()).unwrap();
    let prepared = machine
        .create_cpu(relocade_pc::configuration::cpu(), None)
        .unwrap();
    let (_host, endpoint) = wake_connection(prepared.interrupt_handle());
    let runner = Runner::new(clock.clock(), prepared, endpoint)
        .await
        .unwrap();
    runner
        .run(
            futures_timer::Delay::new(Duration::from_secs(30)),
            async |runner| {
                let mut pc = Pc::with_configuration(
                    runner,
                    Configuration {
                        sound_card: SoundCard::SoundBlaster2,
                    },
                )
                .await
                .unwrap();
                let io = pc.cpu().io();
                let advance = clock.clone();
                io.connect(Rc::new(RefCell::new(ClockStep)), |io| {
                    io.bind(
                        0x80,
                        &PortRead::byte(|_| 0xff),
                        &PortWrite::byte(move |_, value| {
                            advance.advance_by(Duration::from_micros(u64::from(value) * 100));
                        }),
                    );
                });
                let (samples, output) = async_channel::bounded(8);
                if capture {
                    pc.attach_audio(
                        cpal::StreamConfig {
                            channels: 1,
                            sample_rate: 10_000,
                            buffer_size: cpal::BufferSize::Fixed(2),
                        },
                        samples,
                    )
                    .await
                    .unwrap();
                }
                let memory = pc.cpu().memory();
                memory.view().write(0x2000, &[0, 128, 255, 128]).unwrap();
                memory.view().write(0x2100, &[255, 128]).unwrap();
                memory.view().write_u32(0x0f * 4, 0x300).unwrap();
                // IRQ7: preserve AX/DX, acknowledge DSP, count completions, PIC EOI, IRET.
                memory
                    .view()
                    .write(
                        0x300,
                        &[
                            0x50, 0x52, 0xba, 0x2e, 2, 0xec, 0xfe, 6, 0, 7, 0xb0, 0x20, 0xe6, 0x20,
                            0x5a, 0x58, 0xcf,
                        ],
                    )
                    .unwrap();
                let mut code = Vec::new();
                out(&mut code, relocade_pic::MASK_PORT, 0x7f);
                out(&mut code, RESET_PORT, 1);
                out(&mut code, RESET_PORT, 0);
                out(&mut code, 0x80, 1); // Reset initialization reaches 100 us.
                code.extend_from_slice(&[
                    0xba,
                    READ_STATUS_PORT as u8,
                    (READ_STATUS_PORT >> 8) as u8,
                    0xec,
                    0xa8,
                    0x80,
                    0x74,
                    0xfb,
                ]);
                read_to(&mut code, READ_DATA_PORT, 0x701);
                out(&mut code, WRITE_DATA_PORT, 0xe1);
                read_to(&mut code, READ_DATA_PORT, 0x702);
                read_to(&mut code, READ_DATA_PORT, 0x703);
                for value in [0xd1, 0x40, 156] {
                    out(&mut code, WRITE_DATA_PORT, value);
                }
                dma(&mut code, 0x2000, 3);
                out(&mut code, 0x80, 4); // Four samples, completing at 500 us.
                wait_irq(&mut code, 1);
                dma(&mut code, 0x2100, 1);
                out(&mut code, 0x80, 2);
                wait_irq(&mut code, 2);
                memory.view().write(0x1000, &code).unwrap();
                let end = 0x1000 + code.len() as u32;
                let exit = pc.cpu().exit_handle();
                pc.cpu()
                    .install([Hook::host(end, async move || {
                        exit.request();
                    })])
                    .await
                    .unwrap();
                let state = pc.cpu().cpu_state();
                state.set_eip(0x1000);
                state.set_sp(0x8000);
                state.set_interrupts_enabled(true);
                assert_eq!(pc.run().await, ExecutionExit::Host(HostExit::Return));
                let mut result = [0; 4];
                memory.view().read(0x700, &mut result).unwrap();
                assert_eq!(result, [2, 0xaa, 2, 1]);
                if capture {
                    let mut pcm = Vec::new();
                    for _ in 0..3 {
                        pcm.extend(output.recv().await.unwrap());
                    }
                    assert_eq!(pcm, [0.0, 0.0, -1.0, 0.0, 127.0 / 128.0, 0.0]);
                }
            },
        )
        .await
        .expect("Sound Blaster guest timed out");
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn guest_detects_and_restarts_sound_blaster_playback() {
    pollster::block_on(async {
        exercise(false).await;
        exercise(true).await;
    });
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn guest_detects_and_restarts_sound_blaster_playback() {
    exercise(false).await;
    exercise(true).await;
}
