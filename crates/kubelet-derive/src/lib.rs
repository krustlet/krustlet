//! A crate for deriving state machine traits in Kubelet. Right now this crate only consists of a
//! derive macro for the `TransitionTo` trait. In addition to the `derive` attribute, this macro
//! also requires the use of a custom attribute called `transition_to` that specifies the types that
//! can be transitioned to. Not specifying this attribute will result in a compile time error. A
//! simple example of this is below:
//!
//! ```rust,no_run
//! use kubelet_derive::TransitionTo;
//!
//! pub struct VolumeMount;
//! pub struct ImagePullBackoff;
//!
//! #[derive(Default, Debug, TransitionTo)]
//! #[transition_to(VolumeMount, ImagePullBackoff)]
//! pub struct ImagePull;
//!```

extern crate proc_macro;

use crate::proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Attribute, DeriveInput, Error, Generics, Ident, Meta, MetaList};

const ATTRIBUTE_NAME: &str = "transition_to";

#[proc_macro_derive(TransitionTo, attributes(transition_to))]
pub fn derive_transition_to(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let name = input.ident;
    let generics = input.generics;

    let mut transitions = get_transitions(input.attrs);

    // We need to check if we found only one attribute. If it isn't set, we need to error
    if transitions.is_empty() {
        let message = format!(
            "No `{}` attribute found for `{}`. Please specify at least one type to transition to",
            ATTRIBUTE_NAME,
            name.to_string()
        );
        TokenStream::from(Error::new(name.span(), message).to_compile_error())
    } else if transitions.len() > 1 {
        let message = format!(
            "Multiple `{}` attributes found for `{}`. Please specify only one attribute",
            ATTRIBUTE_NAME,
            name.to_string()
        );
        TokenStream::from(Error::new(name.span(), message).to_compile_error())
    } else {
        // We can unwrap here because we already checked length
        generate_impl(transitions.pop().unwrap(), generics, name)
    }
}

fn get_transitions(attrs: Vec<Attribute>) -> Vec<MetaList> {
    let mut filtered = Vec::new();
    for attr in attrs.into_iter() {
        if let Some(parsed) = parse_as_transition_attr(attr) {
            filtered.push(parsed)
        }
    }
    filtered
}

fn parse_as_transition_attr(attr: Attribute) -> Option<MetaList> {
    if let Meta::List(parsed_attr) = attr.parse_meta().ok()? {
        match parsed_attr.path.get_ident() {
            Some(id) if id == ATTRIBUTE_NAME => Some(parsed_attr),
            _ => None,
        }
    } else {
        None
    }
}

fn generate_impl(transitions: MetaList, generics: Generics, name: Ident) -> TokenStream {
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let mut token_stream = TokenStream::new();

    for transition_type in transitions.nested.iter() {
        let expanded = quote! {
            impl #impl_generics kubelet::state::TransitionTo<#transition_type> for #name #ty_generics #where_clause {}
        };
        token_stream.extend(TokenStream::from(expanded));
    }

    token_stream
}
