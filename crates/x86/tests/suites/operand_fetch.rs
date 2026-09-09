use crate::support::machine;
use crate::support::step;
use machine::{check, Exit, Image, Step};
use step::TestModule;

fn image(start: u32, code: &[u8]) -> Image {
    let mut image = Image::new(&[]);
    image.guest.clear();
    image.data(0x3000 + (start & 0xfff), code);
    image.register(56, start);
    for offset in [36, 40, 52] {
        image.register(offset, 0x8000);
    }
    image.map(8, 0x5000, true);
    image.data(0x507f, &[0xa5; 4]);
    image
}

#[test]
fn conditional_operand_fields_respect_the_proven_fetch_extent() {
    let module = TestModule::interpreter();
    for (name, start, code, next) in [
        (
            "SIB and displacement fill the five-byte window",
            0x1ffb,
            &[0xc6, 0x44, 0x24, 0x7f, 0x80][..],
            0x2000,
        ),
        (
            "absent SIB leaves a complete four-byte instruction",
            0x1ffc,
            &[0xc6, 0x43, 0x7f, 0x80][..],
            0x2000,
        ),
    ] {
        check(
            module,
            name,
            &image(start, code),
            &[Step {
                cpu: &[(56, next), (144, 0)],
                ram: &[(0x507f, &[0x80])],
                exit: Exit::Dispatch(next),
            }],
        );
    }
    for (name, start, code) in [
        (
            "missing byte immediate after SIB and displacement",
            0x1ffc,
            &[0xc6, 0x44, 0x24, 0x7f][..],
        ),
        (
            "wide immediate extends beyond the five-byte proof",
            0x1ffb,
            &[0xc7, 0x44, 0x24, 0x7f, 0x12][..],
        ),
    ] {
        check(
            module,
            name,
            &image(start, code),
            &[Step {
                cpu: &[],
                ram: &[],
                exit: Exit::PageFault {
                    address: 0x00002000,
                    error: 0x10,
                },
            }],
        );
    }
    for (name, start) in [
        ("extended opcode retains its direct window", 0x1ffa),
        ("extended opcode uses checked reads at page end", 0x1ffc),
    ] {
        let mut image = image(start, &[0x0f, 0x94, 0x47, 0x7f]);
        image.cpu[0] = 11;
        image.register(4, 0);
        check(
            module,
            name,
            &image,
            &[Step {
                cpu: &[(56, start + 4), (144, 0)],
                ram: &[(0x507f, &[1])],
                exit: Exit::Dispatch(start + 4),
            }],
        );
    }
}
