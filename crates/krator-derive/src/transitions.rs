use crate::proc_macro::TokenStream;
use quote::quote;
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input,
    token::Comma,
    Attribute, DeriveInput, Error, Generics, Ident, Path, Result,
};

const ATTRIBUTE_NAME: &str = "transition_to";

struct Transitions {
    all: Vec<Path>,
}

impl Parse for Transitions {
    fn parse(input: ParseStream) -> Result<Self> {
        Ok(Transitions {
            all: input
                .parse_terminated::<Path, Comma>(|i| i.parse::<Path>())?
                .into_iter()
                .collect(),
        })
    }
}

pub fn run_custom_derive(input: TokenStream) -> TokenStream {
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

fn get_transitions(attrs: Vec<Attribute>) -> Vec<Transitions> {
    attrs
        .into_iter()
        .filter_map(parse_as_transition_attr)
        .collect()
}

fn parse_as_transition_attr(attr: Attribute) -> Option<Transitions> {
    if let Some(id) = attr.path.get_ident() {
        if id == ATTRIBUTE_NAME {
            attr.parse_args::<Transitions>().ok()
        } else {
            None
        }
    } else {
        None
    }
}

fn generate_impl(transitions: Transitions, generics: Generics, name: Ident) -> TokenStream {
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let mut token_stream = TokenStream::new();

    for transition_type in transitions.all.into_iter() {
        let expanded = quote! {
            #[automatically_derived]
            impl#impl_generics krator::TransitionTo<#transition_type> for #name#ty_generics #where_clause {}
        };
        token_stream.extend(TokenStream::from(expanded));
    }

    token_stream
}
