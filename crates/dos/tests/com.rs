use wasm86_dos::{load_com, Dos, DosStop};
use wasm86_runtime::{Runtime, Session, SliceStop};
#[cfg(not(target_arch = "wasm32"))]
use wasm86_runtime_wasmtime::WasmtimeBackend as TestBackend;
#[cfg(target_arch = "wasm32")]
use wasm86_runtime_web::WebBackend as TestBackend;
use wasm86_x86::ExecutionExit;

type Process = Session<TestBackend, Dos>;

async fn process(image: &[u8], tail: &[u8]) -> Process {
    use std::cell::RefCell;
    thread_local! {
        static RUNTIME: RefCell<Option<Runtime<TestBackend>>> = const { RefCell::new(None) };
    }
    let runtime = RUNTIME.with_borrow_mut(|runtime| {
        runtime
            .get_or_insert_with(|| {
                #[cfg(not(target_arch = "wasm32"))]
                let backend = TestBackend::new().unwrap();
                #[cfg(target_arch = "wasm32")]
                let backend = TestBackend;
                Runtime::new(backend)
            })
            .clone()
    });
    runtime
        .attach(load_com(image, tail).unwrap())
        .await
        .unwrap()
}

fn finish(process: &mut Process) -> DosStop {
    let result = process.run_slice(1000).unwrap();
    assert!(matches!(
        result.stop,
        SliceStop::Exited(ExecutionExit::HostYield { .. })
    ));
    process.with_environment(|dos| dos.stop().expect("DOS publishes a reason before yielding"))
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn com_prints_and_exits_through_real_interrupts() {
    let mut process = process(
        &[
            0xba, 0x0c, 0x01, 0xb4, 0x09, 0xcd, 0x21, 0xb8, 42, 0x4c, 0xcd, 0x21, b'H', b'i',
            b'\r', b'\n', b'$',
        ],
        &[],
    )
    .await;
    assert_eq!(finish(&mut process), DosStop::Exited(42));
    assert_eq!(
        process.with_environment(|dos| dos.output().to_vec()),
        b"Hi\r\n"
    );
    let cpu = process.cpu();
    assert_eq!(
        (cpu.segments.cs.selector, cpu.eip, cpu.registers.esp),
        (0xf000, 0x110, 0xfff8)
    );
    assert_eq!(cpu.instruction_count, 6);
    let mut frame = [0; 6];
    process.read_memory(0x1fff8, &mut frame).unwrap();
    assert_eq!(frame, [0x0c, 1, 0, 0x10, 2, 2]);
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn com_layout_and_near_return_follow_the_psp_convention() {
    let mut process = process(&[0xc3], b" /Q").await;
    let cpu = process.cpu();
    assert_eq!(
        (
            cpu.segments.cs.selector,
            cpu.segments.ds.selector,
            cpu.segments.es.selector,
            cpu.segments.ss.selector
        ),
        (0x1000, 0x1000, 0x1000, 0x1000)
    );
    assert_eq!((cpu.eip, cpu.registers.esp), (0x100, 0xfffe));
    let mut psp = [0; 256];
    process.read_memory(0x10000, &mut psp).unwrap();
    assert_eq!(&psp[..4], &[0xcd, 0x20, 0, 0x20]);
    assert_eq!(&psp[0x2c..0x2e], &[0xf0, 0x0f]);
    assert_eq!(&psp[0x80..0x85], &[3, b' ', b'/', b'Q', b'\r']);
    assert_eq!(&psp[0x50..0x53], &[0xcd, 0x21, 0xcb]);
    let mut environment = [0; 19];
    process.read_memory(0xff00, &mut environment).unwrap();
    assert_eq!(&environment, b"\0\0\x01\0C:\\PROGRAM.COM\0");
    assert_eq!(finish(&mut process), DosStop::Exited(0));
    assert_eq!(process.cpu().instruction_count, 2);
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn handle_output_wraps_buffers_and_returns_count_or_carry_through_the_saved_frame() {
    for (handle, expected_ax, expected_cf) in [(1, 4, 0), (3, 6, 1)] {
        let image = [
            0xb8,
            0,
            0x30,
            0x8e,
            0xd8, // DS = 3000h
            0xbb,
            handle,
            0,
            0xb9,
            4,
            0,
            0xba,
            0xfe,
            0xff,
            0xb4,
            0x40,
            if expected_cf == 0 { 0xf9 } else { 0xf8 }, // set the opposite carry
            0xcd,
            0x21,
            0xb8,
            0,
            0x4c,
            0xcd,
            0x21,
        ];
        let mut process = process(&image, &[]).await;
        process.write_memory(0x3fffe, b"A$").unwrap();
        process.write_memory(0x30000, b"\0B").unwrap();
        assert_eq!(process.run_slice(10).unwrap().stop, SliceStop::WorkLimit);
        let cpu = process.cpu();
        assert_eq!(
            (
                cpu.registers.eax,
                cpu.flags.bytes.cf & 1,
                cpu.eip,
                cpu.registers.esp
            ),
            (expected_ax, expected_cf, 0x113, 0xfffe)
        );
        assert_eq!(
            process.with_environment(|dos| dos.output().to_vec()),
            if handle == 1 {
                b"A$\0B".to_vec()
            } else {
                vec![]
            }
        );
        assert_eq!(finish(&mut process), DosStop::Exited(0));
    }
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn dos_vector_services_install_a_guest_handler_that_chains_to_the_kernel() {
    let mut image = vec![
        0xb8, 0x21, 0x35, 0xcd, 0x21, // get INT 21 vector
        0x89, 0x1e, 0x80, 1, 0x8c, 0x06, 0x82, 1, // save ES:BX
        0xba, 0x40, 1, 0xb8, 0x21, 0x25, 0xcd, 0x21, // set DS:0140
        0xb4, 9, 0xba, 0x60, 1, 0xcd, 0x21, 0xb8, 7, 0x4c, 0xcd, 0x21,
    ];
    image.resize(0x40, 0);
    image.extend_from_slice(&[0xff, 0x06, 0x84, 1, 0xff, 0x2e, 0x80, 1]);
    image.resize(0x60, 0);
    image.extend_from_slice(b"X$");
    let mut process = process(&image, &[]).await;
    assert_eq!(finish(&mut process), DosStop::Exited(7));
    assert_eq!(process.with_environment(|dos| dos.output().to_vec()), b"X");
    let mut saved = [0; 6];
    process.read_memory(0x10180, &mut saved).unwrap();
    assert_eq!(saved, [0x10, 1, 0, 0xf0, 2, 0]);
    let mut vector = [0; 4];
    process.read_memory(0x84, &mut vector).unwrap();
    assert_eq!(vector, [0x40, 1, 0, 0x10]);
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn unimplemented_service_yields_a_diagnostic_without_continuing() {
    let mut process = process(&[0xb4, 0x3d, 0xcd, 0x21, 0x40], &[]).await;
    assert_eq!(finish(&mut process), DosStop::UnsupportedFunction(0x3d));
    assert_eq!(process.cpu().instruction_count, 2);
    assert_eq!(process.with_environment(|dos| dos.output().len()), 0);
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn character_and_string_output_preserve_bytes_and_wrap_the_segment_offset() {
    let mut process = process(
        &[
            0xb2, b'>', 0xb4, 2, 0xcd, 0x21, // output character
            0xb8, 0, 0x30, 0x8e, 0xd8, // DS = 3000h
            0xba, 0xfe, 0xff, 0xb4, 9, 0xcd, 0x21, // string at DS:FFFE
            0xb8, 0, 0x4c, 0xcd, 0x21,
        ],
        &[],
    )
    .await;
    process.write_memory(0x3fffe, b"ab").unwrap();
    process.write_memory(0x30000, b"c$").unwrap();
    assert_eq!(process.run_slice(5).unwrap().stop, SliceStop::WorkLimit);
    assert_eq!(process.cpu().registers.eax as u8, b'>');
    assert_eq!(process.run_slice(7).unwrap().stop, SliceStop::WorkLimit);
    assert_eq!(process.cpu().registers.eax as u8, b'$');
    assert_eq!(
        process.with_environment(|dos| dos.output().to_vec()),
        b">abc"
    );
    assert_eq!(finish(&mut process), DosStop::Exited(0));
}

#[cfg_attr(not(target_arch = "wasm32"), pollster::test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
async fn unterminated_string_stops_after_scanning_one_segment() {
    let mut process = process(
        &[0xb8, 0, 0x30, 0x8e, 0xd8, 0x31, 0xd2, 0xb4, 9, 0xcd, 0x21],
        &[],
    )
    .await;
    assert_eq!(
        finish(&mut process),
        DosStop::UnterminatedString {
            segment: 0x3000,
            offset: 0
        }
    );
    assert_eq!(process.with_environment(|dos| dos.output().len()), 0);
}
