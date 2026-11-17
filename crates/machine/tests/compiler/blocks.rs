use super::*;
use relocade_x86::{CodeAddress, Segment, SegmentRegister};

async fn requested_segments(service: &relocade_machine::CompilerService) {
    let (mut machine, config) = setup();
    let first = FarPointer {
        segment: 0x10,
        offset: 0x10,
    };
    let alias = FarPointer {
        segment: 0x11,
        offset: 0,
    };
    let other = FarPointer {
        segment: 0x30,
        offset: 0x10,
    };
    assert_eq!(first.address(), alias.address());
    let entries = [first, alias, other];
    for (entry, immediate) in [(first, 42), (other, 73)] {
        machine
            .memory()
            .view()
            .write(entry.address(), &[0xb8, immediate, 0])
            .unwrap();
    }
    let cpu = machine.create_cpu(config, Some(service)).unwrap();
    cpu.run_until(pending::<()>(), async move |mut runner| {
        let state = runner.cpu().cpu_state();
        let mut compiler = runner.take_compiler().unwrap();
        compiler
            .request(
                state.execution_target(),
                entries
                    .map(|entry| BlockRequest {
                        entry,
                        byte_len: 3,
                        instruction_limit: 1,
                    })
                    .to_vec(),
            )
            .unwrap();

        // The stopped CPU is in a fourth segment when the worker result arrives.
        state.set_segment(Segment::Cs, SegmentRegister::real_mode(Segment::Cs, 0x40));
        let compiled = compiler.recv().await.unwrap().unwrap();
        assert_eq!(
            compiled
                .blocks()
                .iter()
                .map(|block| block.address)
                .collect::<Vec<_>>(),
            entries.map(CodeAddress::from)
        );
        assert_eq!(compiled.ranges(), &[0x110..0x113, 0x310..0x313]);
        runner
            .cpu()
            .install(entries.map(|entry| {
                Hook::host(entry.packed(), async || {
                    panic!("the compiled block must replace this CS:IP hook")
                })
            }))
            .await
            .unwrap();
        compiled.install(runner.cpu()).unwrap();

        for (entry, expected) in [(first, 42), (alias, 42), (other, 73)] {
            state.set_segment(
                Segment::Cs,
                SegmentRegister::real_mode(Segment::Cs, entry.segment),
            );
            state.set_eip(u32::from(entry.offset));
            state.set_ax(0);
            runner
                .cpu()
                .instruction_target()
                .set(Some(state.instruction_count() + 1));
            assert_eq!(runner.run_cpu().await, ExecutionExit::BudgetExhausted);
            assert_eq!(state.ax(), expected);
        }
    })
    .await
    .unwrap()
    .unwrap();
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn block_requests_keep_their_own_code_segments() {
    pollster::block_on(compilation::run(requested_segments));
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn block_requests_keep_their_own_code_segments() {
    compilation::run(requested_segments).await;
}
