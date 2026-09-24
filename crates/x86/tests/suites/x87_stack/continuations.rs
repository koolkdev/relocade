//! Stack writes remain observable across deferred faults, clearing and reset.

use super::*;

fn suppressed_push_preserves_earlier_payload(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for resume in [false, true] {
        let mut code = vec![
            0xd9, 0xc0, // FLD ST0
            0xdd, 0xd8, // FSTP ST0
            0xdd, 0x05, 0, 0x40, 0, 0, // FLD m64 [4000]
        ];
        code.extend_from_slice(if resume {
            &[0xdb, 0xe2, 0xd9, 0xc0] // FNCLEX; FLD ST0
        } else {
            &[0x9b] // FWAIT
        });
        let mut image = initial_image(&code, 0, 0xfffc);
        image.cpu.x87.status = status(0);
        set_control(&mut image.cpu.x87.control, 0x037e);
        image.map(4, 0x8000, false);
        image.data(0x8000, &0x7ff0_0000_0000_0001_u64.to_le_bytes());

        let mut pushed = complete(image.cpu, 2, 0x01c0);
        pushed.x87.status.top = 7;
        pushed.x87.tag_word = 0x3ffc;
        write_register_bits(&mut pushed, 7, register_bits(&image.cpu, 0));
        let mut popped = complete(pushed, 2, 0x05d8);
        popped.x87.status.top = 0;
        popped.x87.tag_word = 0xfffc;
        // The SNaN's unmasked invalid exception leaves the previous payload in
        // physical slot 7, even though that slot is now empty.
        let mut suppressed = complete(popped, 6, 0x0505);
        suppressed.x87.status = status(0x8081);
        suppressed.x87.data_offset = 0x4000;
        suppressed.x87.data_selector = 0x23;
        let mut steps = vec![dispatch(pushed), dispatch(popped), dispatch(suppressed)];
        if resume {
            let mut cleared = suppressed;
            cleared.eip += 2;
            cleared.instruction_count = cleared.instruction_count.wrapping_add(1);
            cleared.x87.status = status(0);
            let mut resumed = complete(cleared, 2, 0x01c0);
            resumed.x87.status.top = 7;
            resumed.x87.tag_word = 0x3ffc;
            steps.extend([dispatch(cleared), dispatch(resumed)]);
        } else {
            steps.push(Step {
                cpu: suppressed,
                ram: &[],
                exit: Exit::FloatingPoint,
            });
        }
        checks.check(
            if resume {
                "FNCLEX resumes from the TOP retained by a suppressed push"
            } else {
                "a suppressed push preserves an earlier write to its empty destination"
            },
            &code,
            &image,
            &steps,
        );
    }
}

fn empty_payloads_survive_reset_and_fault(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for reset in [false, true] {
        let mut code = vec![0xd9, 0xc0, 0xdd, 0xd8]; // FLD ST0; FSTP ST0
        code.extend_from_slice(if reset {
            &[0xdb, 0xe3] // FNINIT
        } else {
            &[0xa1, 0, 0x50, 0, 0] // MOV EAX, [5000], unmapped
        });
        let mut image = initial_image(&code, 3, 0xff3f);
        image.cpu.x87.status = status(0x1800);
        let mut pushed = complete(image.cpu, 2, 0x01c0);
        pushed.x87.status.top = 2;
        pushed.x87.tag_word = 0xff0f;
        write_register_bits(&mut pushed, 2, register_bits(&image.cpu, 3));
        let mut popped = complete(pushed, 2, 0x05d8);
        popped.x87.status.top = 3;
        popped.x87.tag_word = 0xff3f;
        let final_step = if reset {
            let mut initialized = popped;
            initialized.eip += 2;
            initialized.instruction_count = initialized.instruction_count.wrapping_add(1);
            initialized.x87.status = status(0);
            initialized.x87.tag_word = 0xffff;
            initialized.x87.opcode = 0;
            initialized.x87.instruction_offset = 0;
            initialized.x87.instruction_selector = 0;
            initialized.x87.data_offset = 0;
            initialized.x87.data_selector = 0;
            dispatch(initialized)
        } else {
            Step {
                cpu: popped,
                ram: &[],
                exit: Exit::PageFault {
                    address: 0x5000,
                    error: 0,
                },
            }
        };
        checks.check(
            if reset {
                "FNINIT preserves the last payload written to an emptied slot"
            } else {
                "an integer memory fault preserves the last payload in an emptied slot"
            },
            &code,
            &image,
            &[dispatch(pushed), dispatch(popped), final_step],
        );
    }
}

fn self_pop_leaves_empty_source(engine: Engine, frontend: Frontend) {
    let code = [
        0xd9, 0xc0, // FLD ST0
        0xdd, 0xd8, // FSTP ST0
        0xd9, 0xf6, // FDECSTP
        0xd9, 0xc0, // FLD ST0, now empty
    ];
    let mut image = initial_image(&code, 0, 0xfffc);
    image.cpu.x87.status = status(0);
    let mut pushed = complete(image.cpu, 2, 0x01c0);
    pushed.x87.status.top = 7;
    pushed.x87.tag_word = 0x3ffc;
    write_register_bits(&mut pushed, 7, register_bits(&image.cpu, 0));
    let mut popped = complete(pushed, 2, 0x05d8);
    popped.x87.status.top = 0;
    popped.x87.tag_word = 0xfffc;
    let mut rotated = complete(popped, 2, 0x01f6);
    rotated.x87.status.top = 7;
    let mut underflowed = complete(rotated, 2, 0x01c0);
    underflowed.x87.status = status(0x3041);
    underflowed.x87.tag_word = 0xeffc;
    write_register_bits(&mut underflowed, 6, INDEFINITE);
    ImageSequences::new(engine, frontend, SegmentProfile::Flat32).check(
        "FSTP ST0 empties its slot after writing the payload",
        &code,
        &image,
        &[
            dispatch(pushed),
            dispatch(popped),
            dispatch(rotated),
            dispatch(underflowed),
        ],
    );
}

test_frontends!(suppressed_push, suppressed_push_preserves_earlier_payload);
test_frontends!(empty_payloads, empty_payloads_survive_reset_and_fault);
test_frontends!(self_pop, self_pop_leaves_empty_source);
