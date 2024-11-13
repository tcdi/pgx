//LICENSE Portions Copyright 2019-2021 ZomboDB, LLC.
//LICENSE
//LICENSE Portions Copyright 2021-2023 Technology Concepts & Design, Inc.
//LICENSE
//LICENSE Portions Copyright 2023-2023 PgCentral Foundation, Inc. <contact@pgcentral.org>
//LICENSE
//LICENSE All rights reserved.
//LICENSE
//LICENSE Use of this source code is governed by the MIT license that can be found in the LICENSE file.
//! Helper trait for the `#[derive(PostgresType)]` proc macro for overriding custom Postgres type
//! input/output functions.
//!
//! The default implementations use `serde_json` to serialize a custom type to human-readable strings,
//! and `serde_cbor` to serialize internally as a `varlena *` for storage on disk.

use crate::datum::PgVarlena;
use crate::*;
#[doc(hidden)]
pub use serde_json::{from_slice as json_from_slice, to_vec as json_to_vec};
use crate::pg_sys::Oid;
use core::ffi::CStr;

/// `#[derive(Copy, Clone, PostgresType)]` types need to implement this trait to provide the text
/// input/output functions for that type
pub trait PgVarlenaInOutFuncs {
    /// Given a string representation of `Self`, parse it into a `PgVarlena<Self>`.
    ///
    /// It is expected that malformed input will raise an `error!()` or `panic!()`
    fn input(input: &core::ffi::CStr) -> PgVarlena<Self>
    where
        Self: Copy + Sized;

    /// Convert `Self` into text by writing to the supplied `StringInfo` buffer
    fn output(&self, buffer: &mut StringInfo);

    /// If PostgreSQL calls the conversion function with NULL as an argument, what
    /// error message should be generated?
    const NULL_ERROR_MESSAGE: Option<&'static str> = None;
}

/// `#[derive(Serialize, Deserialize, PostgresType)]` types may implement this trait if they prefer
/// a textual representation that isn't JSON
pub trait InOutFuncs {
    /// Given a string representation of `Self`, parse it into `Self`.
    ///
    /// It is expected that malformed input will raise an `error!()` or `panic!()`
    fn input(input: &core::ffi::CStr) -> Self
    where
        Self: Sized;

    /// Convert `Self` into text by writing to the supplied `StringInfo` buffer
    fn output(&self, buffer: &mut StringInfo);

    /// If PostgreSQL calls the conversion function with NULL as an argument, what
    /// error message should be generated?
    const NULL_ERROR_MESSAGE: Option<&'static str> = None;
}

/// `#[derive(Serialize, Deserialize, PostgresType)]` types may implement this trait if they prefer
/// a textual representation that isn't JSON
/// Input function taking three arguments of types `cstring`, `oid`, `integer`. 
pub trait TypmodInOutFuncs {
    /// Given a string representation of `Self`, parse it into `Self`.
    ///
    /// It is expected that malformed input will raise an `error!()` or `panic!()`
    fn input(input: &core::ffi::CStr, oid: Oid, typmod: i32) -> Self
    where
        Self: Sized;

    /// Convert `Self` into text by writing to the supplied `StringInfo` buffer
    fn output(&self, buffer: &mut StringInfo);

    /// The type_modifier_input_function is passed the declared modifier(s) in the form of a cstring array. It must check the values for validity (throwing an error if they are wrong), and if they are correct, return a single non-negative integer value that will be stored as the column “typmod”. 
    fn typmod_in(input: Array<&CStr>) -> i32 ;


    /// If PostgreSQL calls the conversion function with NULL as an argument, what
    /// error message should be generated?
    const NULL_ERROR_MESSAGE: Option<&'static str> = None;
}
