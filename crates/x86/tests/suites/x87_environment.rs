//! Legacy environment layouts and the effects of loading or masking exceptions.

#[path = "x87_environment/effects.rs"]
mod effects;
#[path = "x87_environment/state.rs"]
mod state;

use crate::support::{
    encoding::check_length,
    execution::{test_frontends, Frontend, ImageSequences},
    machine::{dispatch, retire, Exit, Image, Step},
    x87::{set_control, stack_image, status, write_register_bits},
};
use relocade_x86::{CpuState, ExecutionProfile, SegmentProfile, Segments};

#[derive(Clone, Copy, Debug)]
struct Format {
    real: bool,
    wide: bool,
}

impl Format {
    const ALL: [Self; 4] = [
        Self {
            real: false,
            wide: false,
        },
        Self {
            real: false,
            wide: true,
        },
        Self {
            real: true,
            wide: false,
        },
        Self {
            real: true,
            wide: true,
        },
    ];

    fn profile(self) -> ExecutionProfile {
        if self.real {
            ExecutionProfile::Real16
        } else {
            SegmentProfile::Flat32.into()
        }
    }

    fn instruction(self, load: bool, address: u32) -> Vec<u8> {
        let mut code = Vec::new();
        if self.wide == self.real {
            code.push(0x66);
        }
        let reg = if load { 4 } else { 6 };
        code.extend([0xd9, (reg << 3) | if self.real { 6 } else { 5 }]);
        let bytes = address.to_le_bytes();
        code.extend_from_slice(&bytes[..if self.real { 2 } else { 4 }]);
        code
    }

    fn bytes(self, words: [u32; 7]) -> Vec<u8> {
        words
            .into_iter()
            .flat_map(|word| {
                let bytes = word.to_le_bytes();
                bytes[..if self.wide { 4 } else { 2 }].to_vec()
            })
            .collect()
    }

    fn image(self, code: &[u8]) -> Image {
        let mut image = stack_image(code, 5, 0x3f95);
        if self.real {
            image.cpu.segments = Segments::real_mode();
        }
        image.map(4, 0x8000, true);
        image.data(0x8000, &[0xa5; 32]);
        image
    }
}

fn save_formats(frontend: Frontend) {
    // Literal fields from Intel Vol. 1 Figures 8-9 through 8-12. The pointers
    // exercise both selector addition and the split real-address high bits.
    let words = [
        [0x0b5f, 0xeba1, 0x3f00, 0x5678, 0x1357, 0xcdef, 0x2468],
        [
            0xffff_0b5f,
            0xffff_eba1,
            0xffff_3f00,
            0x1234_5678,
            0x0654_1357,
            0x89ab_cdef,
            0xffff_2468,
        ],
        [0x0b5f, 0xeba1, 0x3f00, 0x8be8, 0x5654, 0x146f, 0xe000],
        [
            0xffff_0b5f,
            0xffff_eba1,
            0xffff_3f00,
            0xffff_8be8,
            0x0123_5654,
            0xffff_146f,
            0x089a_e000,
        ],
    ];
    for (format, words) in Format::ALL.into_iter().zip(words) {
        let code = format.instruction(false, 0x4001);
        let mut image = format.image(&code);
        set_control(&mut image.cpu.x87.control, 0x0b5f);
        image.cpu.x87.status = status(0xeba1);
        image.cpu.x87.instruction_selector = 0x1357;
        image.cpu.x87.data_selector = 0x2468;
        let mut cpu = retire(image.cpu, code.len());
        set_control(&mut cpu.x87.control, 0x0b7f);
        cpu.x87.status = status(0x6b21);
        let bytes = format.bytes(words);
        ImageSequences::new(frontend, format.profile()).check(
            &format!("FNSTENV {format:?} saves pre-mask state without waiting"),
            &code,
            &image,
            &[Step {
                cpu,
                ram: &[(0x8001, &bytes)],
                exit: Exit::Dispatch(cpu.eip),
            }],
        );
    }
}

fn register_classes(cpu: &mut CpuState) {
    for (index, value) in [
        (0, 0),
        (0x8000_0000_0000_0000, 0x3fff),
        (0, 0x8000),
        (0x8000_0000_0000_0000, 0x7fff),
        (0x8000_0000_0000_0001, 0x7fff),
        (123, 0x3fff),
        (1, 0),
        (0x8000_0000_0000_0000, 1),
    ]
    .into_iter()
    .enumerate()
    {
        write_register_bits(cpu, index, value);
    }
}

fn load_formats(frontend: Frontend) {
    let words = [
        [0x045a, 0xffca, 0xeaaa, 0x5678, 0x1357, 0xcdef, 0x2468],
        [
            0x1234_045a,
            0x5678_ffca,
            0x9abc_eaaa,
            0x1234_5678,
            0xfe54_1357,
            0x89ab_cdef,
            0xabcd_2468,
        ],
        [0x045a, 0xffca, 0xeaaa, 0x8be8, 0x5e54, 0x146f, 0xefff],
        [
            0x1234_045a,
            0x5678_ffca,
            0x9abc_eaaa,
            0xabcd_8be8,
            0xe123_5e54,
            0xabcd_146f,
            0xf89a_efff,
        ],
    ];
    for (format, words) in Format::ALL.into_iter().zip(words) {
        let code = format.instruction(true, 0x4001);
        let mut image = format.image(&code);
        image.data(0x8001, &format.bytes(words));
        register_classes(&mut image.cpu);
        image.cpu.x87.opcode = 0x0123;
        let mut cpu = retire(image.cpu, code.len());
        set_control(&mut cpu.x87.control, 0x045a);
        cpu.x87.status = status(0x7f4a);
        cpu.x87.tag_word = 0xea91;
        if format.real {
            cpu.x87.instruction_offset = if format.wide {
                0x1235_8be8
            } else {
                0x0005_8be8
            };
            cpu.x87.data_offset = if format.wide {
                0x89ae_146f
            } else {
                0x000e_146f
            };
            cpu.x87.instruction_selector = 0;
            cpu.x87.data_selector = 0;
            cpu.x87.opcode = 0x0654;
        } else {
            cpu.x87.instruction_offset = if format.wide { 0x1234_5678 } else { 0x5678 };
            cpu.x87.data_offset = if format.wide { 0x89ab_cdef } else { 0xcdef };
            cpu.x87.instruction_selector = 0x1357;
            cpu.x87.data_selector = 0x2468;
            if format.wide {
                cpu.x87.opcode = 0x0654;
            }
        }
        ImageSequences::new(frontend, format.profile()).check(
            &format!(
                "FLDENV {format:?} reclassifies occupied registers and ignores reserved fields"
            ),
            &code,
            &image,
            &[dispatch(cpu)],
        );
    }
}

test_frontends!(environment_store_formats, save_formats);
test_frontends!(environment_load_formats, load_formats);

#[test]
fn environment_forms_consume_both_operand_sizes() {
    for format in Format::ALL.into_iter().filter(|format| !format.real) {
        for load in [false, true] {
            check_length(&format.instruction(load, 0x4000));
        }
    }
}
