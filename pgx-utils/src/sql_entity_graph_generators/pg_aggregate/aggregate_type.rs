use proc_macro2::TokenStream as TokenStream2;
use quote::ToTokens;
use syn::{
    parse::{Parse, ParseStream},
    parse_quote, Expr, Type,
};
use super::get_pgx_attr_macro;
use crate::sql_entity_graph_generators::pg_extern::NameMacro;

#[derive(Debug, Clone)]
pub(crate) struct AggregateTypeList {
    pub(crate) found: Vec<AggregateType>,
    pub(crate) original: syn::Type,
}

impl AggregateTypeList {
    pub(crate) fn new(maybe_type_list: syn::Type) -> Result<Self, syn::Error> {
        match &maybe_type_list {
            Type::Tuple(tuple) => {
                let mut coll = Vec::new();
                for elem in &tuple.elems {
                    let parsed_elem = AggregateType::new(elem.clone())?;
                    coll.push(parsed_elem);
                }
                Ok(Self {
                    found: coll,
                    original: maybe_type_list,
                })
            }
            ty => Ok(Self {
                found: vec![AggregateType::new(ty.clone())?],
                original: maybe_type_list,
            }),
        }
    }

    pub(crate) fn entity_tokens(&self) -> Expr {
        let found = self.found.iter().map(|x| x.entity_tokens());
        parse_quote! {
            vec![#(#found),*]
        }
    }
}

impl Parse for AggregateTypeList {
    fn parse(input: ParseStream) -> Result<Self, syn::Error> {
        Self::new(input.parse()?)
    }
}

impl ToTokens for AggregateTypeList {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        self.original.to_tokens(tokens)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct AggregateType {
    pub(crate) ty: Type,
    /// The name, if it exists.
    pub(crate) name: Option<String>,
}

impl AggregateType {
    pub(crate) fn new(ty: syn::Type) -> Result<Self, syn::Error> {
        let name_tokens =  get_pgx_attr_macro("name", &ty);
        let name = match name_tokens {
            Some(tokens) => {
                let name_macro = syn::parse2::<NameMacro>(tokens)
                    .expect("Could not parse `name!()` macro");
                Some(name_macro.ident)
            },
            None => None,
        };
        let retval = Self {
            name,
            ty,
        };
        Ok(retval)
    }

    pub(crate) fn entity_tokens(&self) -> Expr {
        let ty = &self.ty;
        let ty_string = ty.to_token_stream().to_string().replace(" ", "");
        let name = self.name.iter();
        parse_quote! {
            pgx::datum::sql_entity_graph::aggregate::AggregateType {
                ty_source: #ty_string,
                ty_id: core::any::TypeId::of::<#ty>(),
                full_path: core::any::type_name::<#ty>(),
                name: None#( .unwrap_or(Some(#name)) )*,
            }
        }
    }
}

impl ToTokens for AggregateType {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        self.ty.to_tokens(tokens)
    }
}

impl Parse for AggregateType {
    fn parse(input: ParseStream) -> Result<Self, syn::Error> {
        Self::new(input.parse()?)
    }
}
