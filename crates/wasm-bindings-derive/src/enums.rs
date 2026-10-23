use super::*;

pub(super) fn expand(input: &DeriveInput, data: &syn::DataEnum) -> syn::Result<Tokens> {
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &input.generics,
            "Abi expects a concrete enum",
        ));
    }
    if data.variants.is_empty() {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "Abi requires at least one enum variant",
        ));
    }
    let name = &input.ident;
    let mut payload = quote!(());
    let mut lower_arms = Vec::new();
    let mut lift_arms = Vec::new();
    for variant in &data.variants {
        let (_, types) = fields(&variant.fields)?;
        let logical = logical(&types);
        let layout = quote!(<#logical as ::relocade_wasm_bindings::WasmRepr>::Slots);
        payload = quote!(<#payload as ::relocade_wasm_bindings::union::Join<#layout>>::Output);
    }
    for (tag, variant) in data.variants.iter().enumerate() {
        let tag = tag as u32;
        let variant_ident = &variant.ident;
        let (_, types) = fields(&variant.fields)?;
        let vars: Vec<_> = (0..types.len())
            .map(|index| quote::format_ident!("field_{index}"))
            .collect();
        let indices: Vec<_> = (0..types.len()).map(Index::from).collect();
        let tuple = quote!((#(#types,)*));
        let layout = quote!(::relocade_wasm_bindings::slots::Layout<#tuple>);
        let pattern = construct(
            quote!(Self::#variant_ident),
            &variant.fields,
            vars.iter().map(|var| quote!(#var)),
        );
        let packed = quote!(<#layout as ::relocade_wasm_bindings::union::Project<#payload>>::project(
            <#tuple as ::relocade_wasm_bindings::AbiValue>::lower((#(#vars,)*))
        ));
        lower_arms.push(quote!(#pattern => (#tag as i32, #packed)));
        let unpacked = quote!(<#payload as ::relocade_wasm_bindings::union::Project<#layout>>::project(values.1));
        let value = construct(
            quote!(Self::#variant_ident),
            &variant.fields,
            indices.iter().map(|index| quote!(fields.#index)),
        );
        lift_arms.push(quote!(#tag => {
            let fields = <#tuple as ::relocade_wasm_bindings::AbiValue>::lift(#unpacked)?;
            ::core::option::Option::Some(#value)
        }));
    }
    Ok(quote! {
        impl ::relocade_wasm_bindings::AbiType for #name {
            type Logical = (::relocade_wasm_bindings::I32, #payload);
        }
        impl ::relocade_wasm_bindings::Enum for #name { type Payload = #payload; }
        impl ::relocade_wasm_bindings::AbiValue for #name {
            type Value = Self;
            #[inline]
            fn lower(value: Self) -> ::relocade_wasm_bindings::Carrier<Self> { match value { #(#lower_arms,)* } }
            #[inline]
            fn lift(values: ::relocade_wasm_bindings::Carrier<Self>) -> ::core::option::Option<Self> {
                match values.0 as u32 { #(#lift_arms,)* _ => ::core::option::Option::None }
            }
        }
    })
}
