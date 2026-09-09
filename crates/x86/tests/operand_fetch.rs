use wasm86_x86::{compile_interpreter_step, CompiledModule};

#[allow(dead_code)]
#[path = "support/machine.rs"]
mod machine;
#[path = "support/step.rs"]
mod step;
use machine::{check, Exit, Image, Step};
use step::ModuleFile;

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
#[ignore = "requires Node.js with WebAssembly support"]
fn conditional_operand_fields_respect_the_proven_fetch_extent() {
    let module = ModuleFile::new(&compile_interpreter_step().unwrap());
    for flags in [
        &[][..],
        &[
            "--no-liftoff",
            "--no-wasm-lazy-compilation",
            "--no-wasm-tier-up",
        ][..],
    ] {
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
                &module,
                flags,
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
                &module,
                flags,
                name,
                &image(start, code),
                &[Step {
                    cpu: &[],
                    ram: &[],
                    exit: Exit::Fault(0x0004_0010_0000_2000),
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
                &module,
                flags,
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
}
