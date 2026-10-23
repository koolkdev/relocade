use super::compiler_fields;
use crate::input::{construct, fields};
use proc_macro2::TokenStream as Tokens;
use quote::{format_ident, quote};
use syn::{DeriveInput, Fields, Index, Member};

pub(crate) fn expand(input: &DeriveInput, data: &syn::DataEnum) -> syn::Result<Tokens> {
    let name = &input.ident;
    let values_name = format_ident!("{name}Values");
    let case_name = format_ident!("{name}Case");
    let vis = &input.vis;
    let payload = quote!(<#name as ::relocade_wasm_bindings::Enum>::Payload);
    let mut variants = Vec::new();
    let mut clone_arms = Vec::new();
    let mut constructors = Vec::new();
    let mut unpack_values = Vec::new();
    let mut constructor_names = std::collections::HashSet::new();
    let tags = quote!(<#name as ::relocade_wasm_bindings::Enum>::TAGS);
    for (ordinal, variant) in data.variants.iter().enumerate() {
        let tag = quote!(#tags[#ordinal]);
        let variant_ident = &variant.ident;
        let constructor = constructor_name(variant_ident)?;
        if !constructor_names.insert(constructor.to_string()) {
            return Err(syn::Error::new_spanned(
                variant_ident,
                "enum variants must have distinct snake_case constructor names",
            ));
        }
        let (members, types) = fields(&variant.fields)?;
        let fields = &variant.fields;
        let compiler_fields = compiler_fields(fields);
        variants.push(quote!(#variant_ident #compiler_fields));
        let vars: Vec<_> = members
            .iter()
            .enumerate()
            .map(|(index, member)| match member {
                Member::Named(name) => name.clone(),
                Member::Unnamed(_) => format_ident!("field_{index}"),
            })
            .collect();
        let indices: Vec<_> = (0..types.len()).map(Index::from).collect();
        let tuple = quote!((#(#types,)*));
        let pattern = match fields {
            Fields::Named(_) => quote!(#case_name::#variant_ident { #(#vars,)* }),
            _ => construct(
                quote!(#case_name::#variant_ident),
                fields,
                vars.iter().map(|var| quote!(#var)),
            ),
        };
        let cloned = construct(
            quote!(#case_name::#variant_ident),
            fields,
            vars.iter().map(|var| quote!(#var.clone())),
        );
        clone_arms.push(quote!(#pattern => #cloned));
        let layout = quote!(::relocade_wasm_bindings::slots::Layout<#tuple>);
        let packed_values = quote!(<#layout as ::relocade_wasm_bindings::union::Project<#payload, ::relocade_compiler::abi::slots::Ir>>::project(
            ::relocade_compiler::abi::slots::value_slots::<#tuple>((#(#vars,)*))
        ));
        constructors.push(quote! {
            pub fn #constructor(#(#vars: impl ::core::convert::Into<::relocade_compiler::abi::Values<#types>>),*) -> Self {
                #(let #vars = #vars.into();)*
                Self { tag: (#tag).into(), payload: #packed_values }
            }
        });
        let unpacked_values = quote!(<#payload as ::relocade_wasm_bindings::union::Project<#layout, ::relocade_compiler::abi::slots::Ir>>::project(
            self.payload.clone()
        ));
        let symbolic_case = construct(
            quote!(#case_name::#variant_ident),
            fields,
            indices.iter().map(|index| quote!(fields.#index)),
        );
        unpack_values.push(quote!(::core::option::Option::Some(tag) if tag == #tag => {
            let fields = ::relocade_compiler::abi::slots::from_value_slots::<#tuple>(#unpacked_values);
            build(arm, #symbolic_case)
        }));
    }
    Ok(quote! {
        #[doc = "A symbolic enum whose tag is determined at guest runtime."]
        #vis struct #values_name {
            tag: ::relocade_compiler::Val<::relocade_compiler::I32>,
            payload: <#payload as ::relocade_compiler::Results>::Values,
        }
        #[doc = "The active enum case inside a generated match arm."]
        #vis enum #case_name { #(#variants,)* }
        impl ::core::clone::Clone for #values_name {
            fn clone(&self) -> Self {
                Self { tag: self.tag.clone(), payload: self.payload.clone() }
            }
        }
        impl ::core::clone::Clone for #case_name {
            fn clone(&self) -> Self { match self { #(#clone_arms,)* } }
        }
        impl ::core::convert::From<#name> for #values_name {
            fn from(value: #name) -> Self { ::relocade_compiler::abi::constant::<#name>(value) }
        }
        impl #values_name {
            #(#constructors)*
            /// Builds an exhaustive Rust match over the possible runtime cases.
            /// An invalid tag traps. Arms may continue, return, tail-call or trap.
            pub fn match_(self, body: &mut ::relocade_compiler::BlockBuilder<'_>, mut build: impl ::core::ops::FnMut(::relocade_compiler::BlockBuilder<'_>, #case_name) -> ::core::result::Result<(), ::relocade_compiler::BuildError>) -> ::core::result::Result<(), ::relocade_compiler::BuildError> {
                body.switch(&self.tag, #tags, |arm, tag| match tag {
                    #(#unpack_values,)*
                    _ => arm.trap(),
                })
            }
        }
        impl ::relocade_compiler::abi::CompilerValue for #name {
            type Values = #values_name;
            fn from_values(values: <Self::Logical as ::relocade_compiler::Results>::Values) -> Self::Values {
                #values_name { tag: values.0, payload: values.1 }
            }
            fn into_values(values: Self::Values) -> <Self::Logical as ::relocade_compiler::Results>::Values { (values.tag, values.payload) }
        }
        impl ::core::convert::From<#values_name> for ::relocade_compiler::Arguments {
            fn from(values: #values_name) -> Self {
                (values.tag, values.payload).into()
            }
        }
    })
}

fn constructor_name(name: &syn::Ident) -> syn::Result<syn::Ident> {
    let spelling = name.to_string();
    let chars: Vec<_> = spelling.trim_start_matches("r#").chars().collect();
    let mut snake = String::new();
    for (index, &ch) in chars.iter().enumerate() {
        if ch.is_uppercase()
            && index > 0
            && chars[index - 1] != '_'
            && (!chars[index - 1].is_uppercase()
                || chars.get(index + 1).is_some_and(|next| next.is_lowercase()))
        {
            snake.push('_');
        }
        snake.extend(ch.to_lowercase());
    }
    if matches!(snake.as_str(), "self" | "super" | "crate") {
        snake.push('_');
    }
    if snake == "match_" {
        return Err(syn::Error::new_spanned(
            name,
            "this enum variant name conflicts with the generated match_ method",
        ));
    }
    Ok(syn::Ident::new_raw(&snake, name.span()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    #[test]
    fn constructor_names_handle_acronyms_and_rust_keywords() {
        for (variant, method) in [
            ("HTTPStatus", "r#http_status"),
            ("Type", "r#type"),
            ("Match", "r#match"),
            ("Crate", "r#crate_"),
            ("Clone", "r#clone"),
            ("From", "r#from"),
        ] {
            assert_eq!(
                constructor_name(&syn::parse_str(variant).unwrap())
                    .unwrap()
                    .to_string(),
                method,
            );
        }
    }

    #[test]
    fn conflicting_constructor_names_report_the_source_variant() {
        for input in [
            parse_quote!(
                enum Reply {
                    HTTPStatus,
                    HttpStatus,
                }
            ),
            parse_quote!(
                enum Reply {
                    Crate,
                    Crate_,
                }
            ),
            parse_quote!(
                enum Reply {
                    Match_,
                }
            ),
        ] {
            let error = crate::expand(
                &input,
                crate::compiler_value::structs::expand,
                super::expand,
            )
            .unwrap_err()
            .to_string();
            assert!(
                error.contains("constructor names") || error.contains("generated match_ method")
            );
        }
    }
}
