use super::{expand, Binding};
use quote::quote;
use syn::parse_quote;

fn error(binding: proc_macro2::TokenStream) -> String {
    syn::parse2::<Binding>(binding)
        .and_then(|binding| {
            expand(
                binding,
                parse_quote!(
                    fn service(memory: &MemoryView<'_>, value: u16) -> u16 {
                        value
                    }
                ),
            )
        })
        .unwrap_err()
        .to_string()
}

#[test]
fn arguments_must_name_distinct_service_parameters() {
    assert!(error(quote!(args(value = ax, value = bx), returns {})).contains("more than once"));
    assert!(
        error(quote!(context(memory), args(memory = ax), returns {})).contains("more than once")
    );
    assert!(error(quote!(args(missing = ax), returns {})).contains("no argument"));
    assert!(error(quote!(context(value), returns {})).contains("references"));
    for flag in [quote!(cf), quote!(zf), quote!(interrupts_enabled)] {
        assert!(error(quote!(args(value = #flag), returns {})).contains("flags are return values"));
    }
}

#[test]
fn input_mappings_only_read_registers_and_record_fields() {
    for mapping in [
        quote!(ax + 1),
        quote!(reg(ax)),
        quote!(Point { x: cx + 1, y: dx }),
        quote!(Point { x: reg(cx), y: dx }),
    ] {
        assert!(error(quote!(args(value = #mapping))).contains("expected a register"));
    }
    assert!(error(quote!(args(value = Point { x: cx, ..previous })))
        .contains("explicit record of register fields"));
}

#[test]
fn output_flags_cannot_be_assigned_twice_in_one_arm() {
    for flag in [quote!(cf), quote!(zf), quote!(interrupts_enabled)] {
        assert!(
            error(quote!(returns { value => { #flag = false, #flag = true } }))
                .contains("duplicate output flag")
        );
    }
}

#[test]
fn stack_pointer_cannot_replace_the_interrupt_frame() {
    assert!(
        error(quote!(returns { value => { sp = value } })).contains("preserve the stack pointer")
    );
}
