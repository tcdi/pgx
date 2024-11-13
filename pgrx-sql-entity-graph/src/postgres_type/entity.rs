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

`#[derive(PostgresType)]` related entities for Rust to SQL translation

> Like all of the [`sql_entity_graph`][crate] APIs, this is considered **internal**
> to the `pgrx` framework and very subject to change between versions. While you may use this, please do it with caution.

*/
use crate::mapping::RustSqlMapping;
use crate::pgrx_sql::PgrxSql;
use crate::to_sql::entity::ToSqlConfigEntity;
use crate::to_sql::ToSql;
use crate::{SqlGraphEntity, SqlGraphIdentifier, TypeMatch};
use std::collections::BTreeSet;

use eyre::eyre;
/// The output of a [`PostgresType`](crate::postgres_type::PostgresTypeDerive) from `quote::ToTokens::to_tokens`.
#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct PostgresTypeEntity {
    pub name: &'static str,
    pub file: &'static str,
    pub line: u32,
    pub full_path: &'static str,
    pub module_path: &'static str,
    pub mappings: BTreeSet<RustSqlMapping>,
    pub in_fn: &'static str,
    pub in_fn_module_path: String,
    pub out_fn: &'static str,
    pub out_fn_module_path: String,
    pub typmod_in_fn: &'static str,
    pub typmod_in_fn_module_path: String,
    pub to_sql_config: ToSqlConfigEntity,
}

impl TypeMatch for PostgresTypeEntity {
    fn id_matches(&self, candidate: &core::any::TypeId) -> bool {
        self.mappings.iter().any(|tester| *candidate == tester.id)
    }
}

impl From<PostgresTypeEntity> for SqlGraphEntity {
    fn from(val: PostgresTypeEntity) -> Self {
        SqlGraphEntity::Type(val)
    }
}

impl SqlGraphIdentifier for PostgresTypeEntity {
    fn dot_identifier(&self) -> String {
        format!("type {}", self.full_path)
    }
    fn rust_identifier(&self) -> String {
        self.full_path.to_string()
    }

    fn file(&self) -> Option<&'static str> {
        Some(self.file)
    }

    fn line(&self) -> Option<u32> {
        Some(self.line)
    }
}

impl ToSql for PostgresTypeEntity {
    fn to_sql(&self, context: &PgrxSql) -> eyre::Result<String> {
        let self_index = context.types[self];
        let item_node = &context.graph[self_index];
        let SqlGraphEntity::Type(PostgresTypeEntity {
            name,
            file,
            line,
            in_fn_module_path,
            module_path,
            full_path,
            out_fn,
            out_fn_module_path,
            in_fn,
            typmod_in_fn,
            typmod_in_fn_module_path,
            ..
        }) = item_node
        else {
            return Err(eyre!("Was not called on a Type. Got: {:?}", item_node));
        };

        // The `in_fn`/`out_fn` need to be present in a certain order:
        // - CREATE TYPE;
        // - CREATE FUNCTION _in;
        // - CREATE FUNCTION _out;
        // - CREATE TYPE (...);

        let in_fn_module_path = if !in_fn_module_path.is_empty() {
            in_fn_module_path.clone()
        } else {
            module_path.to_string() // Presume a local
        };
        let in_fn_path = format!(
            "{in_fn_module_path}{maybe_colons}{in_fn}",
            maybe_colons = if !in_fn_module_path.is_empty() { "::" } else { "" }
        );
        let (_, _index) = context
            .externs
            .iter()
            .find(|(k, _v)| k.full_path == in_fn_path)
            .ok_or_else(|| eyre::eyre!("Did not find `in_fn: {}`.", in_fn_path))?;
        let (in_fn_graph_index, in_fn_entity) = context
            .graph
            .neighbors_undirected(self_index)
            .find_map(|neighbor| match &context.graph[neighbor] {
                SqlGraphEntity::Function(func) if func.full_path == in_fn_path => {
                    Some((neighbor, func))
                }
                _ => None,
            })
            .ok_or_else(|| eyre!("Could not find in_fn graph entity."))?;
        let in_fn_sql = in_fn_entity.to_sql(context)?;

        let out_fn_module_path = if !out_fn_module_path.is_empty() {
            out_fn_module_path.clone()
        } else {
            module_path.to_string() // Presume a local
        };
        let out_fn_path = format!(
            "{out_fn_module_path}{maybe_colons}{out_fn}",
            maybe_colons = if !out_fn_module_path.is_empty() { "::" } else { "" },
        );
        let (_, _index) = context
            .externs
            .iter()
            .find(|(k, _v)| k.full_path == out_fn_path)
            .ok_or_else(|| eyre::eyre!("Did not find `out_fn: {}`.", out_fn_path))?;
        let (out_fn_graph_index, out_fn_entity) = context
            .graph
            .neighbors_undirected(self_index)
            .find_map(|neighbor| match &context.graph[neighbor] {
                SqlGraphEntity::Function(func) if func.full_path == out_fn_path => {
                    Some((neighbor, func))
                }
                _ => None,
            })
            .ok_or_else(|| eyre!("Could not find out_fn graph entity."))?;
        let out_fn_sql = out_fn_entity.to_sql(context)?;

        let typmod_in_fn_module_path = if !typmod_in_fn_module_path.is_empty() {
            typmod_in_fn_module_path.clone()
        } else {
            module_path.to_string() // Presume a local
        };
        let typmod_in_fn_path = format!(
            "{typmod_in_fn_module_path}{maybe_colons}{typmod_in_fn}",
            maybe_colons = if !typmod_in_fn_module_path.is_empty() { "::" } else { "" }
        );
        let (_, _index) = context
            .externs
            .iter()
            .find(|(k, _v)| k.full_path == typmod_in_fn_path)
            .ok_or_else(|| eyre::eyre!("Did not find `typmod_in_fn: {}`.", typmod_in_fn_path))?;
        let (typmod_in_fn_graph_index, typmod_in_fn_entity) = context
            .graph
            .neighbors_undirected(context.graph_root)
            .find_map(|neighbor| match &context.graph[neighbor] {
                SqlGraphEntity::Function(func) if func.full_path == typmod_in_fn_path => {
                    Some((neighbor, func))
                }
                _ => None,
            })
            .ok_or_else(|| eyre!("Could not find typmod_in_fn graph entity."))?;
        let typmod_in_fn_sql = typmod_in_fn_entity.to_sql(context)?;

        let shell_type = format!(
            "\n\
                -- {file}:{line}\n\
                -- {full_path}\n\
                CREATE TYPE {schema}{name};\
            ",
            schema = context.schema_prefix_for(&self_index),
        );

        let materialized_type = format! {
            "\n\
                -- {file}:{line}\n\
                -- {full_path}\n\
                CREATE TYPE {schema}{name} (\n\
                    \tINTERNALLENGTH = variable,\n\
                    \tINPUT = {schema_prefix_in_fn}{in_fn}, /* {in_fn_path} */\n\
                    \tOUTPUT = {schema_prefix_out_fn}{out_fn}, /* {out_fn_path} */\n\
                    \tTYPMOD_IN = {schema_prefix_typmod_in_fn}{typmod_in_fn}, /* {typmod_in_fn_path} */\n\
                    \tSTORAGE = extended\n\
                );\
            ",
            schema = context.schema_prefix_for(&self_index),
            schema_prefix_in_fn = context.schema_prefix_for(&in_fn_graph_index),
            schema_prefix_out_fn = context.schema_prefix_for(&out_fn_graph_index),
            schema_prefix_typmod_in_fn = context.schema_prefix_for(&typmod_in_fn_graph_index),
        };

        Ok(shell_type + "\n" + &in_fn_sql + "\n" + &out_fn_sql + "\n" + &typmod_in_fn_sql + "\n" + &materialized_type)
    }
}
