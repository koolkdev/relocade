use crate::{CompiledModule, Compiler, ExecutionProfile, SegmentProfile};
use wasmparser::{BranchHint, FunctionBody, KnownCustom, Operator, Parser, Payload, TypeRef};

struct Entry<'a> {
    body: FunctionBody<'a>,
    hints: Vec<BranchHint>,
    dispatch: Option<u32>,
}

fn entry(module: &CompiledModule) -> Entry<'_> {
    let mut entry = None;
    let mut body = None;
    let mut dispatch = None;
    let mut function_index = 0;
    let mut predictions = std::collections::BTreeMap::new();
    for payload in Parser::new(0).parse_all(&module.bytes) {
        match payload.unwrap() {
            Payload::ImportSection(section) => {
                for import in section {
                    let import = import.unwrap();
                    if matches!(import.ty, TypeRef::Func(_)) {
                        if import.module == "wasm86" && import.name == "dispatch" {
                            dispatch = Some(function_index);
                        }
                        function_index += 1;
                    }
                }
            }
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
                            function.hints.into_iter().map(Result::unwrap).collect(),
                        );
                    }
                }
            }
            Payload::CodeSectionEntry(function) => {
                if entry == Some(function_index) {
                    body = Some(function);
                }
                function_index += 1;
            }
            _ => {}
        }
    }
    Entry {
        body: body.expect("compiled entry has a function body"),
        hints: predictions
            .remove(&entry.expect("compiled entry is exported"))
            .unwrap_or_default(),
        dispatch,
    }
}

fn entry_predictions(module: &CompiledModule) -> Vec<bool> {
    entry(module).hints.iter().map(|hint| hint.taken).collect()
}

#[test]
fn conditional_successors_dispatch_inside_separate_branch_arms() {
    let compiler = Compiler::new(SegmentProfile::Flat32);
    for code in [
        &[0x74, 5][..],
        &[0x2e, 0x74, 5],
        &[0x3e, 0x74, 5],
        &[0xe3, 5],
        &[0xe2, 5],
        &[0xe1, 5],
        &[0xe0, 5],
    ] {
        let module = compiler.compile_block(0x1000, code, 1).unwrap();
        let entry = entry(&module);
        let mut controls = Vec::new();
        let mut dispatches = Vec::new();
        for operator in entry
            .body
            .get_operators_reader()
            .unwrap()
            .into_iter_with_offsets()
        {
            let (operator, offset) = operator.unwrap();
            match operator {
                Operator::If { .. } => {
                    controls.push(Some(((offset - entry.body.range().start) as u32, true)));
                }
                Operator::Block { .. } | Operator::Loop { .. } => controls.push(None),
                Operator::Else => controls.last_mut().unwrap().as_mut().unwrap().1 = false,
                Operator::End => {
                    controls.pop();
                }
                Operator::ReturnCall { function_index }
                    if Some(function_index) == entry.dispatch =>
                {
                    dispatches.push(controls.iter().flatten().copied().collect::<Vec<_>>());
                }
                _ => {}
            }
        }
        let [taken, fallthrough] = dispatches.as_slice() else {
            panic!("{code:02x?}: both successor edges must dispatch separately: {dispatches:?}");
        };
        let [(branch, true)] = taken.as_slice() else {
            panic!("{code:02x?}: taken dispatch belongs to the conditional arm: {taken:?}");
        };
        assert_eq!(fallthrough, &[(*branch, false)], "{code:02x?}");
        for hint in entry.hints {
            assert_eq!(hint.func_offset, *branch, "hint predicts the dispatch edge");
        }
    }
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
