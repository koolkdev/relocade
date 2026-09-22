//! Conversion between independent backing fields and architectural control words.

use super::*;

fn control_field_packing(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    let code = [0xd9, 0x3d, 1, 0x40, 0, 0];
    for (fields, bytes) in [
        (
            StoredX87Control {
                invalid_mask: 0x81,
                denormal_mask: 0x82,
                zero_divide_mask: 0x83,
                overflow_mask: 0x84,
                underflow_mask: 0x85,
                precision_mask: 0x86,
                precision_control: 0xfe,
                rounding_control: 0xfd,
                infinity_control: 0x81,
                reserved: 0xa7,
                reserved_bits: 0xffff,
            },
            [0xd5, 0xf6],
        ),
        (
            StoredX87Control {
                invalid_mask: 0x80,
                denormal_mask: 0x81,
                zero_divide_mask: 0x82,
                overflow_mask: 0x83,
                underflow_mask: 0x84,
                precision_mask: 0x85,
                precision_control: 0xfd,
                rounding_control: 0xfe,
                infinity_control: 0x82,
                reserved: 0xb8,
                reserved_bits: 0x1f3f,
            },
            [0x2a, 0x09],
        ),
    ] {
        let mut image = initial_image(&code);
        image.cpu.x87.control = fields;
        image.map(4, 0x8000, true);
        image.data(0x8000, &[0x11, 0x22, 0x33, 0x44]);
        let observed = retire(image.cpu, 6);
        checks.check(
            "FNSTCW packs architectural bits without modifying raw backing bytes",
            &code,
            &image,
            &[Step {
                cpu: observed,
                ram: &[(0x8001, &bytes)],
                exit: Exit::Dispatch(observed.eip),
            }],
        );
    }
}

fn load_control_fields(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    let code = [
        0xd9, 0x2d, 0, 0x40, 0, 0, // FLDCW [4000]
        0xd9, 0x3d, 2, 0x40, 0, 0, // FNSTCW [4002]
    ];
    for (bytes, fields) in [
        (
            [0xff, 0xff],
            StoredX87Control {
                invalid_mask: 1,
                denormal_mask: 1,
                zero_divide_mask: 1,
                overflow_mask: 1,
                underflow_mask: 1,
                precision_mask: 1,
                precision_control: 3,
                rounding_control: 3,
                infinity_control: 1,
                reserved: 0x5a,
                reserved_bits: 0xe0c0,
            },
        ),
        (
            [0, 0],
            StoredX87Control {
                invalid_mask: 0,
                denormal_mask: 0,
                zero_divide_mask: 0,
                overflow_mask: 0,
                underflow_mask: 0,
                precision_mask: 0,
                precision_control: 0,
                rounding_control: 0,
                infinity_control: 0,
                reserved: 0x5a,
                reserved_bits: 0,
            },
        ),
    ] {
        let mut image = initial_image(&code);
        image.cpu.x87.status = status(0x3a00);
        image.cpu.x87.control.reserved = 0x5a;
        image.map(4, 0x8000, true);
        image.data(0x8000, &[bytes[0], bytes[1], 0xaa, 0xbb]);
        let mut loaded = retire(image.cpu, 6);
        loaded.x87.control = fields;
        let observed = retire(loaded, 6);
        checks.check(
            "FLDCW replaces architectural fields, preserves padding and round trips the word",
            &code,
            &image,
            &[
                dispatch(loaded),
                Step {
                    cpu: observed,
                    ram: &[(0x8002, &bytes)],
                    exit: Exit::Dispatch(observed.eip),
                },
            ],
        );
    }
}

test_frontends!(control_fields_pack_at_observation, control_field_packing);
test_frontends!(
    control_loads_replace_architectural_fields,
    load_control_fields
);
