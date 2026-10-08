use super::*;
use crate::support::{
    blocks::BlockModules,
    machine::expected,
    step::{Event, TestModule},
};
use wasm86_x86::{compile_interpreter, SegmentProfile};

fn scalar(engine: Engine, frontend: Frontend) {
    let mut blocks = BlockModules::default();
    for (code, width, immediate, output) in [
        (&[0xe4, 0x92][..], 1, true, false),
        (&[0xe5, 0xff][..], 2, true, false),
        (&[0x66, 0xe5, 0xff][..], 4, true, false),
        (&[0xec][..], 1, false, false),
        (&[0xed][..], 2, false, false),
        (&[0x66, 0xed][..], 4, false, false),
        (&[0xe6, 0x92][..], 1, true, true),
        (&[0xe7, 0xff][..], 2, true, true),
        (&[0x66, 0xe7, 0xff][..], 4, true, true),
        (&[0xee][..], 1, false, true),
        (&[0xef][..], 2, false, true),
        (&[0x66, 0xef][..], 4, false, true),
    ] {
        let mut image = image(code);
        image.cpu.registers.eax = 0x89ab_cdef;
        image.cpu.registers.edx = 0xabcd_ffff;
        let module = match frontend {
            Frontend::Block => blocks.get(&image.cpu, code, 8, ExecutionProfile::Real16),
            Frontend::Interpreter => TestModule::interpreter_with_profile(ExecutionProfile::Real16),
        };
        let mut input = image.input();
        let mask = u32::MAX >> (32 - width * 8);
        let mut cpu = retired(&image, code.len());
        let port = if immediate {
            *code.last().unwrap() as u32
        } else {
            0xffff
        };
        let event = if output {
            Event::PortWrite {
                port,
                bytes: width,
                value: image.cpu.registers.eax & mask,
            }
        } else {
            input.port_reads = vec![0x7654_3210];
            cpu.registers.eax = (cpu.registers.eax & !mask) | (0x7654_3210 & mask);
            Event::PortRead { port, bytes: width }
        };
        let mut wanted = expected(
            &image,
            &[Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(cpu.eip),
            }],
        );
        wanted.events.insert(0, event);
        assert_eq!(engine.observe(module, &input, 1), wanted);
    }
}
test_frontends!(scalar_port_transfers, scalar);

fn boundaries(engine: Engine) {
    let run = TestModule::new(&compile_interpreter(ExecutionProfile::Real16).unwrap());
    let code = [0xe4, 0x92, 0xb8, 0, 0];
    let image = image(&code);
    let mut input = image.input();
    input.port_reads = vec![0xff];
    let mut cpu = retired(&image, 2);
    cpu.registers.eax = (cpu.registers.eax & !0xff) | 0xff;
    let mut wanted = expected(
        &image,
        &[Step {
            cpu,
            ram: &[],
            exit: Exit::Dispatch(cpu.eip),
        }],
    );
    wanted.events.insert(
        0,
        Event::PortRead {
            port: 0x92,
            bytes: 1,
        },
    );
    let mut blocks = BlockModules::default();
    let block = blocks.get(&image.cpu, &code, 8, ExecutionProfile::Real16);
    for module in [&run, block] {
        assert_eq!(
            engine.observe(module, &input, 1),
            wanted,
            "port read stays observable and ends execution before the overwrite"
        );
    }
    for profile in [
        SegmentProfile::Flat32,
        SegmentProfile::Segmented32,
        SegmentProfile::Segmented16,
    ] {
        for code in [&[0xe4, 0x92][..], &[0xef][..]] {
            let mut image = super::image(code);
            image.cpu.segments = if profile == SegmentProfile::Segmented16 {
                let mut segments = Segments::flat32();
                segments.cs.attributes = SegmentAttributes::from_bits(7);
                segments
            } else {
                Segments::flat32()
            };
            let block = blocks.get(&image.cpu, code, 8, profile);
            for module in [block, TestModule::interpreter_with_profile(profile)] {
                image.check_unchanged_exit(
                    engine,
                    module,
                    "fixed user profiles deny I/O",
                    Exit::GeneralProtection { error: 0 },
                );
            }
        }
    }
    for code in [&[0xf0, 0xec][..], &[0xf0, 0xe6, 0x92][..]] {
        let image = super::image(code);
        image.check_unchanged_exit(
            engine,
            TestModule::interpreter_with_profile(ExecutionProfile::Real16),
            "LOCK port I/O follows the shared unsupported-encoding path",
            Exit::Other(0x0008_00f0_0000_1000),
        );
    }
}
#[test]
fn port_boundaries() {
    boundaries(Engine::Wasmtime);
}
#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_port_boundaries() {
    boundaries(Engine::V8);
}
