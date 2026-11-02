//! ABI declarations on the service implementation; selection belongs to its table.

mod argument;
mod pointer;
mod register;
mod segment;
#[cfg(test)]
mod tests;

use proc_macro2::{Span, TokenStream as Tokens};
use quote::{format_ident, quote};
use register::Register;
use std::collections::{BTreeMap, BTreeSet};
use syn::{
    braced, parenthesized,
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
    token,
    visit_mut::{self, VisitMut},
    Expr, FnArg, Ident, ItemFn, Lifetime, Pat, Signature, Token, Type, Visibility,
};

pub(super) struct Binding {
    context: Vec<Ident>,
    args: Vec<Argument>,
    returns: Vec<ReturnArm>,
}
struct Argument {
    name: Ident,
    value: Expr,
}
struct ReturnArm {
    pattern: Pat,
    action: ReturnAction,
}
enum ReturnAction {
    Values(Vec<Output>),
    Service(Expr),
}
struct Output {
    destination: Register,
    value: Expr,
}

impl Parse for Binding {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut context = None;
        let mut args = None;
        let mut returns = None;
        while !input.is_empty() {
            let section: Ident = input.parse()?;
            match section.to_string().as_str() {
                "context" if context.is_none() => {
                    let content;
                    parenthesized!(content in input);
                    context = Some(
                        Punctuated::<Ident, Token![,]>::parse_terminated(&content)?
                            .into_iter()
                            .collect(),
                    );
                }
                "args" if args.is_none() => {
                    let content;
                    parenthesized!(content in input);
                    args = Some(
                        Punctuated::<Argument, Token![,]>::parse_terminated(&content)?
                            .into_iter()
                            .collect(),
                    );
                }
                "returns" if returns.is_none() => {
                    let content;
                    braced!(content in input);
                    returns = Some(
                        Punctuated::<ReturnArm, Token![,]>::parse_terminated(&content)?
                            .into_iter()
                            .collect(),
                    );
                }
                _ => {
                    return Err(syn::Error::new_spanned(
                        section,
                        "expected context(...), args(...) or returns {...}, each at most once",
                    ))
                }
            }
            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
        }
        Ok(Self {
            context: context.unwrap_or_default(),
            args: args.unwrap_or_default(),
            returns: returns.unwrap_or_else(|| vec![syn::parse_quote!(() => {})]),
        })
    }
}
impl Parse for Argument {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let name = input.parse()?;
        input.parse::<Token![=]>()?;
        Ok(Self {
            name,
            value: input.parse()?,
        })
    }
}
impl Parse for ReturnArm {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let pattern = Pat::parse_multi_with_leading_vert(input)?;
        input.parse::<Token![=>]>()?;
        let action = if input.peek(token::Brace) {
            let content;
            braced!(content in input);
            ReturnAction::Values(
                Punctuated::<Output, Token![,]>::parse_terminated(&content)?
                    .into_iter()
                    .collect(),
            )
        } else {
            ReturnAction::Service(input.parse()?)
        };
        Ok(Self { pattern, action })
    }
}
impl Parse for Output {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let destination = input.parse()?;
        input.parse::<Token![=]>()?;
        Ok(Self {
            destination,
            value: input.parse()?,
        })
    }
}

pub(super) fn expand(binding: Binding, function: ItemFn) -> syn::Result<Tokens> {
    let signature = &function.sig;
    if signature.constness.is_some()
        || signature.unsafety.is_some()
        || signature.abi.is_some()
        || signature.variadic.is_some()
        || !signature.generics.params.is_empty()
    {
        return Err(syn::Error::new_spanned(
            signature,
            "interrupt bindings require a concrete Rust function",
        ));
    }
    let mut parameters = BTreeMap::new();
    for arg in &signature.inputs {
        if let FnArg::Typed(arg) = arg {
            let Pat::Ident(name) = &*arg.pat else {
                return Err(syn::Error::new_spanned(arg, "expected a named argument"));
            };
            if name.by_ref.is_some() || name.subpat.is_some() {
                return Err(syn::Error::new_spanned(
                    name,
                    "expected a plain argument name",
                ));
            }
            parameters.insert(name.ident.to_string(), (&name.ident, &*arg.ty));
        }
    }
    let mut mapped = BTreeSet::new();
    for name in binding
        .context
        .iter()
        .chain(binding.args.iter().map(|arg| &arg.name))
    {
        if !parameters.contains_key(&name.to_string()) {
            return Err(syn::Error::new_spanned(name, "no argument with this name"));
        }
        if !mapped.insert(name.to_string()) {
            return Err(syn::Error::new_spanned(
                name,
                "argument is mapped more than once",
            ));
        }
    }
    let call = Ident::new("__interrupt_call", Span::mixed_site());
    let cpu = Ident::new("__interrupt_cpu", Span::mixed_site());
    let result = Ident::new("__interrupt_result", Span::mixed_site());
    let values = Ident::new("__interrupt_values", Span::mixed_site());
    let mut supplied = BTreeMap::new();
    for name in &binding.context {
        let (_, ty) = parameters[&name.to_string()];
        let Type::Reference(reference) = ty else {
            return Err(syn::Error::new_spanned(
                ty,
                "context inputs must be references",
            ));
        };
        let borrow = &reference.mutability;
        let value = match name.to_string().as_str() {
            "memory" => quote!(&#borrow #call.memory),
            "io" | "cpu" => quote!(#call.#name),
            "call" => quote!(&#borrow *#call),
            _ => {
                return Err(syn::Error::new_spanned(
                    name,
                    "context inputs are memory, io, cpu, or call",
                ))
            }
        };
        supplied.insert(name.to_string(), value);
    }
    let mut reads = Vec::new();
    for arg in &binding.args {
        let name = &arg.name;
        let (_, ty) = parameters[&name.to_string()];
        let value = argument::read(&arg.value, &cpu)?;
        reads.push(quote!(let #name: #ty = { let #cpu = #call.cpu; #value };));
        supplied.insert(name.to_string(), quote!(#name));
    }
    let mut entry = signature.clone();
    entry.ident = format_ident!("{}_entry", signature.ident);
    entry.inputs.clear();
    let method = matches!(signature.inputs.first(), Some(FnArg::Receiver(_)));
    if method {
        entry.inputs.push(signature.inputs.first().unwrap().clone());
    }
    entry
        .inputs
        .push(syn::parse_quote!(#call: &mut ::relocade_machine::CpuContext<'_>));
    let mut arguments = Vec::new();
    for arg in &signature.inputs {
        if let FnArg::Typed(arg) = arg {
            let Pat::Ident(name) = &*arg.pat else {
                unreachable!()
            };
            let name = &name.ident;
            arguments.push(
                supplied
                    .get(&name.to_string())
                    .cloned()
                    .unwrap_or_else(|| quote!(#name)),
            );
            if !mapped.contains(&name.to_string()) {
                entry.inputs.push(FnArg::Typed(arg.clone()));
            }
        }
    }
    let mut arms = Vec::new();
    let mut checks = Vec::new();
    for arm in &binding.returns {
        let pattern = &arm.pattern;
        let action = match &arm.action {
            ReturnAction::Service(service) => {
                let invoke = crate::interrupt_dispatch::call_entry(service.clone(), &call)?;
                quote!(#invoke.await)
            }
            ReturnAction::Values(outputs) => {
                let mut flags = BTreeSet::new();
                let mut ranges = Vec::new();
                let mut expressions = Vec::new();
                let mut writes = Vec::new();
                for (index, output) in outputs.iter().enumerate() {
                    if let Register::Flag(name) = &output.destination {
                        if !flags.insert(name.to_string()) {
                            return Err(syn::Error::new_spanned(name, "duplicate output flag"));
                        }
                    }
                    ranges.extend(output.destination.ranges());
                    expressions.push(&output.value);
                    let index = syn::Index::from(index);
                    writes.push(output.destination.write(&call, quote!(#values.#index))?);
                }
                checks.push(quote!(::relocade_machine::real_mode_interrupt::registers::assert_disjoint(&[#(#ranges),*]);));
                quote!({ let #values = (#(#expressions,)*); #(#writes)* })
            }
        };
        arms.push(quote!(#pattern => #action));
    }
    let name = &signature.ident;
    let invoke = if method {
        quote!(self.#name(#(#arguments),*))
    } else {
        quote!(self::#name(#(#arguments),*))
    };
    let await_call = signature.asyncness.map(|_| quote!(.await));
    let visibility = match &function.vis {
        Visibility::Public(_) => syn::parse_quote!(pub(crate)),
        vis => vis.clone(),
    };
    let attributes = function
        .attrs
        .iter()
        .filter(|attr| {
            attr.path().is_ident("cfg")
                || attr.path().is_ident("cfg_attr")
                || attr.path().is_ident("allow")
        })
        .collect::<Vec<_>>();
    bind_future(&mut entry);
    Ok(quote! {
        #function
        #(#attributes)*
        #visibility #entry {
            const _: () = { #(#checks)* };
            #(#reads)*
            async move {
                let #result = #invoke #await_call;
                match #result { #(#arms,)* }
            }
        }
    })
}

/// Name input borrows so Rust 2021 can capture them in a returned future without
/// tying independent service/context lifetimes together.
pub(super) fn bind_future(signature: &mut Signature) {
    #[derive(Default)]
    struct Borrows(Vec<Lifetime>);
    impl VisitMut for Borrows {
        fn visit_type_reference_mut(&mut self, reference: &mut syn::TypeReference) {
            reference
                .lifetime
                .get_or_insert_with(|| syn::parse_quote!('_));
            visit_mut::visit_type_reference_mut(self, reference);
        }

        fn visit_receiver_mut(&mut self, receiver: &mut syn::Receiver) {
            if receiver.colon_token.is_none() {
                if let Some((_, lifetime)) = &mut receiver.reference {
                    let lifetime = lifetime.get_or_insert_with(|| syn::parse_quote!('_));
                    self.visit_lifetime_mut(lifetime);
                    let mutability = receiver.mutability;
                    receiver.ty = Box::new(syn::parse_quote!(&#lifetime #mutability Self));
                }
            } else {
                visit_mut::visit_receiver_mut(self, receiver);
            }
        }

        fn visit_lifetime_mut(&mut self, lifetime: &mut Lifetime) {
            if lifetime.ident == "_" {
                *lifetime = Lifetime::new(
                    &format!("'__interrupt_borrow_{}", self.0.len()),
                    Span::mixed_site(),
                );
                self.0.push(lifetime.clone());
            }
        }
    }
    let mut borrows = Borrows::default();
    for parameter in &mut signature.inputs {
        borrows.visit_fn_arg_mut(parameter);
    }
    let lifetimes = borrows.0;
    let inputs = signature.inputs.iter().map(|parameter| match parameter {
        FnArg::Receiver(receiver) => &receiver.ty,
        FnArg::Typed(parameter) => &parameter.ty,
    });
    signature.asyncness = None;
    signature.generics = syn::parse_quote!(<#(#lifetimes),*>);
    signature.output = syn::parse_quote!(
        -> impl ::core::future::Future<Output = ()>
            + ::relocade_machine::real_mode_interrupt::Captures<(#(#inputs,)*)>
    );
}
