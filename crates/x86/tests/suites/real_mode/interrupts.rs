use super::*;
use crate::support::{
    blocks::BlockModules,
    machine::expected,
    step::{Event, TestModule},
};

fn interrupt_image(code: &[u8], vector: u8) -> Image {
    let mut image = image(code);
    image.cpu.flags.status_source.kind = 0;
    image.cpu.segments.ss = cache(Segment::Ss, 0x2000);
    image.cpu.registers.esp = 0xabcd_1006;
    image.map(0, 0x7000, false);
    image.map(0x21, 0x8000, true);
    image.data(0x7000 + u32::from(vector) * 4, &[0xff; 4]);
    image
}

fn entered(image: &Image, length: usize) -> CpuState {
    let mut cpu = retired(image, length);
    cpu.eip = 0xffff;
    cpu.segments.cs = cache(Segment::Cs, 0xffff);
    cpu.registers.esp -= 6;
    cpu.flags.bytes.if_ = 0;
    cpu.flags.bytes.tf = 0;
    cpu.flags.bytes.ac = 0;
    cpu
}

fn frames(engine: Engine, frontend: Frontend) {
    let mut cases = sequences(engine, frontend);
    for (code, vector) in [
        (&[0xcd, 0][..], 0),
        (&[0xcd, 0xff][..], 0xff),
        (&[0xcc][..], 3),
        (&[0xcd, 3][..], 3),
        (&[0xce][..], 4),
        (&[0x64, 0x66, 0x67, 0xcd, 0x21][..], 0x21),
        (&[0x66, 0xcc][..], 3),
        (&[0x66, 0xce][..], 4),
    ] {
        let mut image = interrupt_image(code, vector);
        image.cpu.segments.cs = cache(Segment::Cs, 0x10);
        image.cpu.segments.ds = cache(Segment::Ds, 0x8000);
        image.cpu.segments.fs = cache(Segment::Fs, 0x9000);
        image.data(0x3100, code);
        // IF does not gate software interrupts, and noncanonical flag bytes survive.
        image.cpu.flags.bytes.if_ = 0x80;
        let cpu = entered(&image, code.len());
        let frame = [code.len() as u8, 0x10, 0x10, 0, 0xd7, 0x5d];
        cases.check(
            "software interrupts save three words, ignore size/segment overrides and clear IF/TF/AC",
            code,
            &image,
            &[Step {
                cpu,
                ram: &[(0x8000, &frame)],
                exit: Exit::Dispatch(0xffff),
            }],
        );
    }
}
test_frontends!(word_frames_and_prefixes, frames);

fn return_ip_wraps(engine: Engine, frontend: Frontend) {
    let code = [0x66, 0xcc];
    let mut image = interrupt_image(&code, 3);
    image.cpu.eip = 0xfffe;
    image.map(0xf, 0x3000, false);
    image.data(0x3ffe, &code);
    let cpu = entered(&image, code.len());
    sequences(engine, frontend).check(
        "the saved return IP wraps after an instruction ending at the CS limit",
        &code,
        &image,
        &[Step {
            cpu,
            ram: &[(0x8000, &[0, 0, 0, 0, 0xd7, 0x5f])],
            exit: Exit::Dispatch(cpu.eip),
        }],
    );
}
test_frontends!(return_ip_is_a_word, return_ip_wraps);

fn stack_bounds(engine: Engine, frontend: Frontend) {
    let mut blocks = BlockModules::default();
    for sp in 1..6 {
        let code = [0x66, 0xcc];
        let mut image = interrupt_image(&code, 3);
        image.cpu.registers.esp = 0xabcd_0000 | sp;
        let mut input = image.input();
        input.physical_pages.retain(|&(page, _, _)| page != 0);
        input.mmio_pages = vec![(0, 0x7000), (0x2f, 0x9000)];
        input.observe_mmio = true;
        let module = match frontend {
            Frontend::Block => blocks.get(&image.cpu, &code, 1, ExecutionProfile::Real16),
            Frontend::Interpreter => TestModule::interpreter_with_profile(ExecutionProfile::Real16),
        };
        assert_eq!(
            engine.observe(module, &input, 1),
            expected(
                &image,
                &[Step {
                    cpu: image.cpu,
                    ram: &[],
                    exit: Exit::StackFault { error: 0 },
                }]
            ),
            "the complete frame must fit before any stack write or vector read"
        );
    }

    for sp in [0, 6] {
        let code = [0xcc];
        let mut image = interrupt_image(&code, 3);
        image.cpu.registers.esp = 0xabcd_0000 | sp;
        let (page, backing) = if sp == 0 {
            (0x2f, 0x8ffa)
        } else {
            (0x20, 0x8000)
        };
        image.map(page, 0x8000, true);
        let mut cpu = entered(&image, code.len());
        cpu.registers.esp = 0xabcd_0000 | (sp.wrapping_sub(6) & 0xffff);
        sequences(engine, frontend).check(
            "a complete frame at either stack boundary preserves the upper ESP word",
            &code,
            &image,
            &[Step {
                cpu,
                ram: &[(backing, &[1, 0x10, 0, 0, 0xd7, 0x5f])],
                exit: Exit::Dispatch(0xffff),
            }],
        );
    }
}
test_frontends!(complete_stack_capacity, stack_bounds);

fn conditional_entry(engine: Engine, frontend: Frontend) {
    let mut blocks = BlockModules::default();
    for overflow in [false, true] {
        let code = [0xce];
        let mut image = interrupt_image(&code, 4);
        image.cpu.flags.status_source.kind = 5; // Pending word subtraction.
        image.cpu.flags.status_source.left = if overflow { 0x8000 } else { 7 };
        image.cpu.flags.status_source.right = if overflow { 1 } else { 8 };
        if !overflow {
            image.cpu.registers.esp = 0xabcd_0001; // Would fault if entry were attempted.
        }
        let mut input = image.input();
        input
            .physical_pages
            .retain(|&(page, _, _)| page != 0 && page != 0x21);
        input.mmio_pages = vec![(0, 0x7000), (0x21, 0x8000)];
        input.observe_mmio = true;
        let cpu = if overflow {
            entered(&image, 1)
        } else {
            retired(&image, 1)
        };
        let frame = [1, 0x10, 0, 0, 0x16, 0x5f];
        let ram = if overflow {
            &[(0x8000, frame.as_slice())][..]
        } else {
            &[]
        };
        let mut wanted = expected(
            &image,
            &[Step {
                cpu,
                ram,
                exit: Exit::Dispatch(cpu.eip),
            }],
        );
        if overflow {
            wanted.events.splice(
                0..0,
                [
                    Event::MmioWrite {
                        address: 0x21004,
                        value: vec![0x16, 0x5f],
                    },
                    Event::MmioWrite {
                        address: 0x21002,
                        value: vec![0, 0],
                    },
                    Event::MmioWrite {
                        address: 0x21000,
                        value: vec![1, 0x10],
                    },
                    Event::MmioRead {
                        address: 18,
                        bytes: 2,
                    },
                    Event::MmioRead {
                        address: 16,
                        bytes: 2,
                    },
                ],
            );
        }
        let module = match frontend {
            Frontend::Block => blocks.get(&image.cpu, &code, 1, ExecutionProfile::Real16),
            Frontend::Interpreter => TestModule::interpreter_with_profile(ExecutionProfile::Real16),
        };
        assert_eq!(engine.observe(module, &input, 1), wanted,
            "INTO tests lazy OF, performs word transfers in order, and skips all entry effects when clear");
    }
}
test_frontends!(conditional_entry_and_mmio, conditional_entry);

fn aliased_vector(engine: Engine, frontend: Frontend) {
    let code = [0xcc];
    let mut image = interrupt_image(&code, 3);
    image.cpu.segments.ss = cache(Segment::Ss, 0);
    image.cpu.registers.esp = 0xabcd_0012;
    image.physical_pages.retain(|&(page, _, _)| page != 0);
    image.map(0, 0x7000, true);
    let mut cpu = entered(&image, 1);
    cpu.segments.cs = cache(Segment::Cs, 0);
    cpu.eip = 0x1001;
    sequences(engine, frontend).check(
        "the return IP and CS overwrite vector 3 before the target is loaded",
        &code,
        &image,
        &[Step {
            cpu,
            ram: &[(0x700c, &[1, 0x10, 0, 0, 0xd7, 0x5f])],
            exit: Exit::Dispatch(0x1001),
        }],
    );
}
test_frontends!(stack_aliases_vector_table, aliased_vector);

fn round_trip(engine: Engine) {
    let code = [0xcd, 0x21];
    let mut image = interrupt_image(&code, 0x21);
    image.data(0x7084, &[0x20, 0, 0, 0x10]);
    image.map(0x10, 0x9000, false);
    image.data(0x9020, &[0xcf]);
    let mut entered = entered(&image, 2);
    entered.eip = 0x20;
    entered.segments.cs = cache(Segment::Cs, 0x1000);
    let mut returned = entered;
    returned.eip = 0x1002;
    returned.segments.cs = image.cpu.segments.cs;
    returned.registers.esp = image.cpu.registers.esp;
    returned.instruction_count = returned.instruction_count.wrapping_add(1);
    for flag in [
        &mut returned.flags.bytes.cf,
        &mut returned.flags.bytes.pf,
        &mut returned.flags.bytes.af,
        &mut returned.flags.bytes.zf,
        &mut returned.flags.bytes.sf,
        &mut returned.flags.bytes.of,
        &mut returned.flags.bytes.tf,
        &mut returned.flags.bytes.if_,
        &mut returned.flags.bytes.df,
        &mut returned.flags.bytes.nt,
        &mut returned.flags.bytes.iopl,
    ] {
        *flag = 1;
    }
    assert_eq!(
        engine.observe(
            TestModule::interpreter_with_profile(ExecutionProfile::Real16),
            &image.input(),
            2
        ),
        expected(
            &image,
            &[
                Step {
                    cpu: entered,
                    ram: &[(0x8000, &[2, 0x10, 0, 0, 0xd7, 0x5f])],
                    exit: Exit::Dispatch(0x20)
                },
                Step {
                    cpu: returned,
                    ram: &[],
                    exit: Exit::Dispatch(0x1002)
                },
            ]
        ),
        "INT dispatches through the new CS and IRET restores its word frame; AC stays clear"
    );
}

#[test]
fn interrupt_return_round_trip() {
    round_trip(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_interrupt_return_round_trip() {
    round_trip(Engine::V8);
}

fn prior_work(engine: Engine, frontend: Frontend) {
    let code = [0xb8, 0x34, 0x12, 0xce];
    for (overflow, fault) in [(false, false), (true, false), (true, true)] {
        let mut image = interrupt_image(&code, 4);
        image.cpu.flags.bytes.of = u8::from(overflow);
        if fault {
            image.cpu.registers.esp = 0xabcd_0001;
        }
        let mut moved = retired(&image, 3);
        moved.registers.eax = 0x1111_1234;
        let mut cpu = if fault {
            moved
        } else if overflow {
            entered(&image, 4)
        } else {
            retired(&image, 4)
        };
        cpu.registers.eax = moved.registers.eax;
        cpu.instruction_count = moved.instruction_count.wrapping_add(u64::from(!fault));
        let frame = [4, 0x10, 0, 0, 0xd7, 0x5f];
        let ram = if overflow && !fault {
            &[(0x8000, frame.as_slice())][..]
        } else {
            &[]
        };
        sequences(engine, frontend).check(
            "conditional transfer publishes prior work and retires only completed instructions",
            &code,
            &image,
            &[
                Step {
                    cpu: moved,
                    ram: &[],
                    exit: Exit::Dispatch(0x1003),
                },
                Step {
                    cpu,
                    ram,
                    exit: if fault {
                        Exit::StackFault { error: 0 }
                    } else {
                        Exit::Dispatch(cpu.eip)
                    },
                },
            ],
        );
    }
}
test_frontends!(conditional_transfer_publication, prior_work);

fn protected_modes(engine: Engine, frontend: Frontend) {
    use wasm86_x86::SegmentProfile;
    for profile in [
        SegmentProfile::Flat32,
        SegmentProfile::Segmented32,
        SegmentProfile::Segmented16,
    ] {
        let mut cases = ImageSequences::new(engine, frontend, profile);
        for (code, opcode) in [
            (&[0xcd, 0x21][..], 0xcd_u64),
            (&[0xcc][..], 0xcc),
            (&[0xce][..], 0xce),
        ] {
            let mut image = Image::new(code);
            if profile == SegmentProfile::Segmented16 {
                image.cpu.segments.cs.attributes = SegmentAttributes::from_bits(7);
            }
            cases.check(
                "protected interrupt delivery exits unsupported before any effects",
                code,
                &image,
                &[Step {
                    cpu: image.cpu,
                    ram: &[],
                    exit: Exit::Other(0x0008_0000_0000_1000 | opcode << 32),
                }],
            );
        }
    }
}
test_frontends!(protected_delivery_is_unsupported, protected_modes);

#[test]
fn decoding_requires_the_vector_and_stops_at_the_transfer() {
    use wasm86_x86::{compile_block_from_bytes_with_profile, BlockError, SegmentProfile};
    for profile in [ExecutionProfile::Real16, SegmentProfile::Flat32.into()] {
        assert!(matches!(
            compile_block_from_bytes_with_profile(0x1000, &[0xcd], 1, profile),
            Err(BlockError::TruncatedInstruction {
                address: 0x1000,
                available: 1
            })
        ));
        for code in [&[0xcd, 0x21][..], &[0xcc], &[0xce]] {
            let complete = compile_block_from_bytes_with_profile(0x1000, code, 1, profile).unwrap();
            let mut trailing = code.to_vec();
            trailing.push(0x0f);
            assert_eq!(
                complete.bytes,
                compile_block_from_bytes_with_profile(0x1000, &trailing, 2, profile)
                    .unwrap()
                    .bytes
            );
        }
    }
}
