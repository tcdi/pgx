//LICENSE Portions Copyright 2019-2021 ZomboDB, LLC.
//LICENSE
//LICENSE Portions Copyright 2021-2023 Technology Concepts & Design, Inc.
//LICENSE
//LICENSE Portions Copyright 2023-2023 PgCentral Foundation, Inc. <contact@pgcentral.org>
//LICENSE
//LICENSE All rights reserved.
//LICENSE
//LICENSE Use of this source code is governed by the MIT license that can be found in the LICENSE file.
/*!

`#[pg_cast]` related macro expansion for Rust to SQL translation

> Like all of the [`sql_entity_graph`][crate] APIs, this is considered **internal**
> to the `pgrx` framework and very subject to change between versions. While you may use this, please do it with caution.

*/
use proc_macro2::TokenStream as TokenStream2;
use quote::{quote, ToTokens, TokenStreamExt};

/// A parsed `#[pg_cast]` operator.
///
/// It is created during [`PgExtern`](crate::PgExtern) parsing.
#[derive(Debug, Clone, Default)]
pub enum PgCast {
    #[default]
    Default,
    Assignment,
    Implicit,
}

impl ToTokens for PgCast {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        let quoted = match self {
            PgCast::Default => quote! {
                ::pgrx::pgrx_sql_entity_graph::PgCastEntity::Default
            },
            PgCast::Assignment => quote! {
                ::pgrx::pgrx_sql_entity_graph::PgCastEntity::Assignment
            },
            PgCast::Implicit => quote! {
                ::pgrx::pgrx_sql_entity_graph::PgCastEntity::Implicit
            },
        };
        tokens.append_all(quoted);
    }
}
