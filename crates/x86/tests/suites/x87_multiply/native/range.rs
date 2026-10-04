//! Native coefficients retain wide exponents and restart at the extreme range boundary.

use super::*;

fn native_extended_range(engine: Engine) {
    // An exact multiplication by one establishes the native significand before
    // the second operation exercises the extended exponent boundaries.
    let code = [0xdc, 0x0d, 0, 0x40, 0, 0, 0xdc, 0x0d, 8, 0x40, 0, 0];
    let compiled = compiler().compile(0x1000, &code, 2).unwrap();
    let module = TestModule::new(&compiled);
    assert_native_multiply(&module);
    let linked = TestModule::new(&compiled).with_interpreter(TestModule::interpreter());
    for (left, multiplier, product, flags, restart) in [
        // Even an exact lowest-binade result deliberately uses the interpreter.
        (
            (LEADING, 1),
            0x3ff0_0000_0000_0000_u64,
            (LEADING, 1),
            0,
            true,
        ),
        (
            (LEADING, 1),
            0x3fe0_0000_0000_0000,
            (LEADING >> 1, 0),
            0,
            true,
        ),
        // Rounding up from below minimum normal still needs the full response.
        (
            (LEADING + 0x800, 1),
            0x3fef_ffff_ffff_fffe,
            (LEADING, 1),
            PE | C1,
            true,
        ),
        // The next binade admits both exact results and a rounding carry into it.
        ((LEADING, 1), 0x4000_0000_0000_0000, (LEADING, 2), 0, false),
        (
            (LEADING + 0x800, 1),
            0x3fff_ffff_ffff_fffe,
            (LEADING, 2),
            PE | C1,
            false,
        ),
        // Binary64's range boundaries do not constrain a normalized coefficient.
        (
            (LEADING, 0x3c01),
            0x3fef_ffff_ffff_ffff,
            (0xffff_ffff_ffff_f800, 0x3c00),
            0,
            false,
        ),
        (
            (LEADING, 0x43fe),
            0x4000_0000_0000_0000,
            (LEADING, 0x43ff),
            0,
            false,
        ),
        (
            (LEADING, 0x7ffe),
            0x3ff0_0000_0000_0000,
            (LEADING, 0x7ffe),
            0,
            false,
        ),
        (
            (LEADING + 0x800, 0x7ffe),
            0x3fff_ffff_ffff_fffe,
            (LEADING, 0x7fff),
            8 | PE | C1,
            true,
        ),
    ] {
        let mut image = stack_image(&code, 0, 0xfffc);
        set_control(&mut image.cpu.x87.control, 0x027f);
        image.cpu.x87.status.precision = 0;
        write_value(&mut image.cpu, 0, left);
        image.map(4, 0x8000, false);
        image.data(0x8000, &0x3ff0_0000_0000_0000_u64.to_le_bytes());
        image.data(0x8008, &multiplier.to_le_bytes());
        let mut first = complete_x87(image.cpu, 6, 0x040d);
        first.x87.status.c1 = 0;
        first.x87.data_offset = 0x4000;
        first.x87.data_selector = 0x23;
        let mut result = complete_x87(first, 6, 0x040d);
        result.x87.data_offset = 0x4008;
        result.x87.status.overflow = u8::from(flags & 8 != 0);
        result.x87.status.precision = u8::from(flags & PE != 0);
        result.x87.status.c1 = u8::from(flags & C1 != 0);
        write_value(&mut result, 0, product);
        let block_exit = if restart {
            Step {
                cpu: first,
                ram: &[],
                exit: Exit::Interpret,
            }
        } else {
            dispatch(result)
        };
        assert_eq!(
            engine.observe(&module, &image.input(), 1),
            expected(&image, &[block_exit])
        );
        if restart {
            assert_eq!(
                engine.observe(&linked, &image.input(), 1),
                expected(&image, &[dispatch(result)])
            );
        }
    }
}

#[test]
fn native_products_preserve_extended_range_and_boundary_restart() {
    native_extended_range(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_native_products_preserve_extended_range_and_boundary_restart() {
    native_extended_range(Engine::V8);
}
