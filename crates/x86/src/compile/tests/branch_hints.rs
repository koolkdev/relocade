use crate::{CompiledModule, Compiler, ExecutionProfile, SegmentProfile};
use wasmparser::{KnownCustom, Parser, Payload};

fn entry_predictions(module: &CompiledModule) -> Vec<bool> {
    let mut entry = None;
    let mut predictions = std::collections::BTreeMap::new();
    for payload in Parser::new(0).parse_all(&module.bytes) {
        match payload.unwrap() {
            Payload::ExportSection(section) => {
                for export in section {
                    let export = export.unwrap();
                    if export.name == module.entry {
                        entry = Some(export.index);
                    }
                }
            }
            Payload::CustomSection(section) => {
                if let KnownCustom::BranchHints(functions) = section.as_known() {
                    for function in functions {
                        let function = function.unwrap();
                        predictions.insert(
                            function.func,
                            function
                                .hints
                                .into_iter()
                                .map(|hint| hint.unwrap().taken)
                                .collect(),
                        );
                    }
                }
            }
            _ => {}
        }
    }
    predictions
        .remove(&entry.expect("compiled entry is exported"))
        .unwrap_or_default()
}

#[test]
fn jcc_prefixes_predict_the_taken_edge_in_short_and_near_forms() {
    let compiler = Compiler::new(SegmentProfile::Flat32);
    for (prefix, taken) in [(0x2e, false), (0x3e, true)] {
        for suffix in [
            &[0x74, 5][..],
            &[0x0f, 0x84, 5, 0, 0, 0],
            &[0x66, 0x0f, 0x84, 5, 0],
        ] {
            let code = [&[prefix][..], suffix].concat();
            let module = compiler.compile_block(0x1000, &code, 1).unwrap();
            assert_eq!(entry_predictions(&module), [taken], "{code:02x?}");
        }
    }
    for code in [&[0x74, 5][..], &[0xe3, 5], &[0xe2, 5]] {
        let module = compiler.compile_block(0x1000, code, 1).unwrap();
        assert!(entry_predictions(&module).is_empty(), "{code:02x?}");
    }
}

#[test]
fn instruction_faults_memory_faults_and_fetch_fallbacks_are_unlikely() {
    for (profile, code) in [
        (
            ExecutionProfile::Protected(SegmentProfile::Flat32),
            &[0xf7, 0xf1][..],
        ), // DIV ECX
        (
            ExecutionProfile::Protected(SegmentProfile::Flat32),
            &[0x8b, 0x03],
        ), // MOV EAX,[EBX]
        (
            ExecutionProfile::Protected(SegmentProfile::Segmented32),
            &[0x90],
        ), // CS fetch guard
        (ExecutionProfile::Real16, &[0xce]), // INTO's conditional interrupt
    ] {
        let module = Compiler::new(profile)
            .compile_block(0x1000, code, 1)
            .unwrap();
        let predictions = entry_predictions(&module);
        assert!(!predictions.is_empty(), "{profile:?}, {code:02x?}");
        assert!(
            predictions.iter().all(|taken| !taken),
            "{profile:?}, {code:02x?}"
        );
    }
}
