use convert_case::{Case, Casing};
use darling::ast::{self};
use darling::FromDeriveInput;
use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::{parse_macro_input, DeriveInput, Generics, Ident};

#[derive(Debug, FromDeriveInput)]
#[darling(attributes(roci), supports(struct_named))]
pub struct Decomponentize {
    ident: Ident,
    generics: Generics,
    data: ast::Data<(), crate::Field>,
}

pub fn decomponentize(input: TokenStream) -> TokenStream {
    let crate_name = crate::roci_crate_name();
    let input = parse_macro_input!(input as DeriveInput);
    let Decomponentize {
        ident,
        generics,
        data,
    } = Decomponentize::from_derive_input(&input).unwrap();
    let where_clause = &generics.where_clause;
    let conduit = quote! { #crate_name::conduit };
    let fields = data.take_struct().unwrap();
    let if_arms = fields.fields.iter().map(|field| {
        let ty = &field.ty;
        let component_id = match &field.component_id {
            Some(c) => quote! {
                #crate_name::conduit::ComponentId::new(#c)
            },
            None => {
                quote! {
                    #crate_name::conduit::ComponentId::new(<#ty as #crate_name::conduit::Component>::NAME)
                }
            },
        };
        let ident = &field.ident;
        let name = field
            .ident
            .as_ref()
            .expect("only named field allowed")
            .to_string()
            .to_case(Case::UpperSnake);
        let id = field.entity_id;
        let const_name = format!("{name}_ID");
        let const_name = syn::Ident::new(&const_name, Span::call_site());
        quote! {
            const #const_name: #conduit::ComponentId = #component_id;
            if component_id == #const_name {
                let payload = payload.as_ref();
                let mut iter = payload.into_iter(metadata.component_type.clone());
                while let Some(Ok(#conduit::ser_de::ColumnValue { entity_id, value })) = iter.next() {
                    if entity_id == EntityId(#id) {
                        if let Some(val) = <#ty>::from_component_value(value) {
                            self.#ident = val;
                        }
                    }
                }
            }
        }
    });
    quote! {
        impl #crate_name::Decomponentize for #ident #generics #where_clause {
            fn apply_column<B: AsRef<[u8]>>(&mut self, metadata: &#conduit::Metadata, payload: &#conduit::ColumnPayload<B>) {
                let component_id = metadata.component_id();
                #(#if_arms)*
            }
        }
    }
    .into()
}
