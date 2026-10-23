//! Read emitted Wasm without deciding which code shape a test must require.

use wasmparser::{FunctionBody, Import, Operator, Parser, Payload, TypeRef};

pub(crate) fn function_bodies(bytes: &[u8]) -> impl Iterator<Item = FunctionBody<'_>> {
    Parser::new(0)
        .parse_all(bytes)
        .filter_map(|payload| match payload.unwrap() {
            Payload::CodeSectionEntry(body) => Some(body),
            _ => None,
        })
}

pub(crate) fn operators(bytes: &[u8]) -> impl Iterator<Item = Operator<'_>> {
    function_bodies(bytes).flat_map(|body| {
        body.get_operators_reader()
            .unwrap()
            .into_iter()
            .map(Result::unwrap)
    })
}

pub(crate) fn imports(bytes: &[u8]) -> impl Iterator<Item = Import<'_>> {
    Parser::new(0)
        .parse_all(bytes)
        .filter_map(|payload| match payload.unwrap() {
            Payload::ImportSection(imports) => Some(imports),
            _ => None,
        })
        .flatten()
        .map(Result::unwrap)
}

pub(crate) fn function_import_index(bytes: &[u8], module: &str, name: &str) -> Option<u32> {
    imports(bytes)
        .filter(|import| matches!(import.ty, TypeRef::Func(_)))
        .position(|import| import.module == module && import.name == name)
        .map(|index| index as u32)
}

pub(crate) fn memory_import_index(bytes: &[u8], module: &str, name: &str) -> Option<u32> {
    imports(bytes)
        .filter(|import| matches!(import.ty, TypeRef::Memory(_)))
        .position(|import| import.module == module && import.name == name)
        .map(|index| index as u32)
}
