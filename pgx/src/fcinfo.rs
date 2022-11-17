/*
Portions Copyright 2019-2021 ZomboDB, LLC.
Portions Copyright 2021-2022 Technology Concepts & Design, Inc. <support@tcdi.com>

All rights reserved.

Use of this source code is governed by the MIT license that can be found in the LICENSE file.
*/

//! Helper macros and functions for creating Postgres UDFs.
//!
//! Other than the exported macros, typically these functions are not necessary to call directly
//! as they're used behind the scenes by the code generated by the `#[pg_extern]` macro.
use crate::{pg_sys, void_mut_ptr, FromDatum, PgBox, PgMemoryContexts};

/// A macro for specifying default argument values so they get propery translated to SQL in
/// `CREATE FUNCTION` statements
///
/// ## Examples
///
/// This example will create a SQL function like so:
///
/// ```sql
/// CREATE FUNCTION fun_with_default_arg_value(a integer, b integer DEFAULT 99) RETURNS integer ...;
/// ```
///
/// ```rust
/// use pgx::prelude::*;
///
/// #[pg_extern]
/// fn fun_with_default_arg_value(a: i32, b: default!(i32, 99)) -> i32 {
///    a + b
/// }
/// ```
///
/// This allows users of this function, from within Postgres, to elide the `b` argument, and
/// Postgres will automatically use `99`.
#[macro_export]
macro_rules! default {
    ($ty:ty, $val:tt) => {
        $ty
    };
    ($ty:ty, $val:path) => {
        $ty
    };
    ($ty:ty, $val:expr) => {
        $ty
    };
}

/// The equivalent of a PostgreSQL `NULL`.
///
/// This is used primarily in `default!()` macros.
pub struct NULL;

/// A macro for providing SQL names for the returned fields for functions that return a Rust tuple,
/// especially those that return a `std::iter::Iterator<Item=(f1, f2, f3)>`
///
/// ## Examples
///
/// This example will create a SQL function like so:
///
/// ```sql
/// CREATE FUNCTION get_a_set() RETURNS TABLE (id integer, title text) ...;
/// ```
///
/// ```rust
/// use pgx::prelude::*;
///
/// #[pg_extern]
/// fn get_a_set() -> TableIterator<'static, (name!(id, i32), name!(title, &'static str))> {
///     TableIterator::new(vec![1, 2, 3].into_iter().zip(vec!["A", "B", "C"].into_iter()))
/// }
/// ```
#[macro_export]
macro_rules! name {
    ($name:tt, $ty:ty) => {
        $ty
    };
}

#[macro_export]
macro_rules! variadic {
    ($ty:ty) => {
        $ty
    };
}

#[cfg(any(feature = "pg11"))]
mod pg_11 {
    use crate::{pg_sys, FromDatum};

    #[inline]
    pub fn pg_getarg<T: FromDatum>(fcinfo: pg_sys::FunctionCallInfo, num: usize) -> Option<T> {
        let datum = unsafe { fcinfo.as_ref() }.unwrap().arg[num];
        let isnull = pg_arg_is_null(fcinfo, num);
        unsafe {
            if T::GET_TYPOID {
                T::from_polymorphic_datum(datum, isnull, super::pg_getarg_type(fcinfo, num))
            } else {
                T::from_datum(datum, isnull)
            }
        }
    }

    #[inline]
    pub fn pg_arg_is_null(fcinfo: pg_sys::FunctionCallInfo, num: usize) -> bool {
        unsafe { fcinfo.as_ref() }.unwrap().argnull[num] as bool
    }

    #[inline]
    pub fn pg_getarg_datum(fcinfo: pg_sys::FunctionCallInfo, num: usize) -> Option<pg_sys::Datum> {
        if pg_arg_is_null(fcinfo, num) {
            None
        } else {
            Some(unsafe { fcinfo.as_ref() }.unwrap().arg[num])
        }
    }

    #[inline]
    pub fn pg_getarg_datum_raw(fcinfo: pg_sys::FunctionCallInfo, num: usize) -> pg_sys::Datum {
        unsafe { fcinfo.as_ref() }.unwrap().arg[num]
    }

    #[inline]
    pub fn pg_return_null(fcinfo: pg_sys::FunctionCallInfo) -> pg_sys::Datum {
        unsafe { fcinfo.as_mut() }.unwrap().isnull = true;
        pg_sys::Datum::from(0)
    }
}

#[cfg(any(feature = "pg12", feature = "pg13", feature = "pg14", feature = "pg15"))]
mod pg_12_13_14_15 {
    use crate::{pg_sys, FromDatum};

    #[inline]
    pub fn pg_getarg<T: FromDatum>(fcinfo: pg_sys::FunctionCallInfo, num: usize) -> Option<T> {
        let datum = get_nullable_datum(fcinfo, num);
        unsafe {
            if T::GET_TYPOID {
                T::from_polymorphic_datum(
                    datum.value,
                    datum.isnull,
                    super::pg_getarg_type(fcinfo, num),
                )
            } else {
                T::from_datum(datum.value, datum.isnull)
            }
        }
    }

    #[inline]
    pub fn pg_arg_is_null(fcinfo: pg_sys::FunctionCallInfo, num: usize) -> bool {
        get_nullable_datum(fcinfo, num).isnull
    }

    #[inline]
    pub fn pg_getarg_datum(fcinfo: pg_sys::FunctionCallInfo, num: usize) -> Option<pg_sys::Datum> {
        if pg_arg_is_null(fcinfo, num) {
            None
        } else {
            Some(get_nullable_datum(fcinfo, num).value)
        }
    }

    #[inline]
    pub fn pg_getarg_datum_raw(fcinfo: pg_sys::FunctionCallInfo, num: usize) -> pg_sys::Datum {
        get_nullable_datum(fcinfo, num).value
    }

    #[inline]
    fn get_nullable_datum(fcinfo: pg_sys::FunctionCallInfo, num: usize) -> pg_sys::NullableDatum {
        let fcinfo = unsafe { fcinfo.as_mut() }.unwrap();
        unsafe {
            let nargs = fcinfo.nargs;
            let len = std::mem::size_of::<pg_sys::NullableDatum>() * nargs as usize;
            fcinfo.args.as_slice(len)[num].clone()
        }
    }

    #[inline]
    pub fn pg_return_null(fcinfo: pg_sys::FunctionCallInfo) -> pg_sys::Datum {
        let fcinfo = unsafe { fcinfo.as_mut() }.unwrap();
        fcinfo.isnull = true;
        pg_sys::Datum::from(0)
    }
}

//
// common
//

#[cfg(any(feature = "pg11"))]
pub use pg_11::*;

#[cfg(any(feature = "pg12", feature = "pg13", feature = "pg14", feature = "pg15"))]
pub use pg_12_13_14_15::*;

#[inline]
pub fn pg_getarg_pointer<T>(fcinfo: pg_sys::FunctionCallInfo, num: usize) -> Option<*mut T> {
    match pg_getarg_datum(fcinfo, num) {
        Some(datum) => Some(datum.cast_mut_ptr::<T>()),
        None => None,
    }
}

/// # Safety
///
/// The provided `fcinfo` must be valid otherwise this function results in undefined behavior due
/// to an out of bounds read.
#[inline]
pub unsafe fn pg_getarg_type(fcinfo: pg_sys::FunctionCallInfo, num: usize) -> pg_sys::Oid {
    pg_sys::get_fn_expr_argtype(fcinfo.as_ref().unwrap().flinfo, num as std::os::raw::c_int)
}

/// this is intended for Postgres functions that take an actual `cstring` argument, not for getting
/// a varlena argument type as a CStr.
#[inline]
pub fn pg_getarg_cstr<'a>(
    fcinfo: pg_sys::FunctionCallInfo,
    num: usize,
) -> Option<&'a std::ffi::CStr> {
    match pg_getarg_pointer(fcinfo, num) {
        Some(ptr) => Some(unsafe { std::ffi::CStr::from_ptr(ptr) }),
        None => None,
    }
}

#[inline]
pub fn pg_return_void() -> pg_sys::Datum {
    pg_sys::Datum::from(0)
}

/// Retrieve the `.flinfo.fn_extra` pointer (as a PgBox'd type) from [`pg_sys::FunctionCallInfo`].
///
/// This function is unsafe as we cannot guarantee the provided [`pg_sys::FunctionCallInfo`] pointer is valid
pub unsafe fn pg_func_extra<ReturnType, DefaultValue: FnOnce() -> ReturnType>(
    fcinfo: pg_sys::FunctionCallInfo,
    default: DefaultValue,
) -> PgBox<ReturnType> {
    let fcinfo = PgBox::from_pg(fcinfo);
    let mut flinfo = PgBox::from_pg(fcinfo.flinfo);
    if flinfo.fn_extra.is_null() {
        flinfo.fn_extra = PgMemoryContexts::For(flinfo.fn_mcxt).leak_and_drop_on_delete(default())
            as void_mut_ptr;
    }

    PgBox::from_pg(flinfo.fn_extra as *mut ReturnType)
}

/// This mimics the functionality of Postgres' `DirectFunctionCall` macros, allowing you to call
/// internal Postgres functions using its "V1" calling convention.  Unlike the Postgres' C macros,
/// the function is allowed to return a NULL datum.
///
/// ## Safety
///
/// This function is unsafe as the underlying function being called is likely unsafe
///
/// ## Examples
/// ```rust
/// use pgx::*;
/// use std::ffi::CString;
///
/// fn some_func() {
///     let result = unsafe {
///         direct_function_call::<pg_sys::Oid>(
///             pg_sys::regclassin,
///             vec![ CString::new("pg_class").unwrap().as_c_str().into_datum()]
///         )
///     };
///     let oid = result.expect("failed to lookup oid for pg_class");
///     assert_eq!(oid, 1259);  // your value could be different, maybe
/// }
/// ```
pub unsafe fn direct_function_call<R: FromDatum>(
    func: unsafe fn(pg_sys::FunctionCallInfo) -> pg_sys::Datum,
    args: Vec<Option<pg_sys::Datum>>,
) -> Option<R> {
    direct_function_call_as_datum(func, args).map_or(None, |d| R::from_datum(d, false))
}

/// Akin to [direct_function_call], but specifically for calling those functions declared with the
/// `#[pg_extern]` attribute.
///
/// When using this, you'll want to suffix the function you want to call with `_wrapper`.
///
/// ## Example
///
/// ```rust,no_run
/// use pgx::*;
///
/// #[pg_extern]
/// fn add_numbers(a: i32, b: i32) -> i32 {
///     a + b
/// }
///
/// /* NOTE:  _wrapper suffix! */
/// let result = unsafe {
///     direct_pg_extern_function_call::<i32>(add_numbers_wrapper, vec![1_i32.into_datum(), 2_i32.into_datum()])
/// };
/// let sum = result.expect("add_numbers_wrapper returned NULL");
/// assert_eq!(3, sum);
/// ```
///
/// ## Safety
///
/// This function is unsafe as the function you're calling is also unsafe
pub unsafe fn direct_pg_extern_function_call<R: FromDatum>(
    func: unsafe extern "C" fn(pg_sys::FunctionCallInfo) -> pg_sys::Datum,
    args: Vec<Option<pg_sys::Datum>>,
) -> Option<R> {
    direct_pg_extern_function_call_as_datum(func, args).map_or(None, |d| R::from_datum(d, false))
}

/// Same as [direct_function_call] but instead returns the direct `Option<pg_sys::Datum>` instead
/// of converting it to a value
///
/// ## Safety
///
/// This function is unsafe as the function you're calling is also unsafe
pub unsafe fn direct_function_call_as_datum(
    func: unsafe fn(pg_sys::FunctionCallInfo) -> pg_sys::Datum,
    args: Vec<Option<pg_sys::Datum>>,
) -> Option<pg_sys::Datum> {
    direct_function_call_as_datum_internal(|fcinfo| func(fcinfo), args)
}

#[cfg(feature = "pg11")]
unsafe fn direct_function_call_as_datum_internal(
    func: impl FnOnce(pg_sys::FunctionCallInfo) -> pg_sys::Datum,
    args: Vec<Option<pg_sys::Datum>>,
) -> Option<pg_sys::Datum> {
    let fcinfo_ptr = pg_sys::palloc(std::mem::size_of::<pg_sys::FunctionCallInfoData>())
        .cast::<pg_sys::FunctionCallInfoData>();

    let fcinfo = fcinfo_ptr.as_mut().unwrap_unchecked();
    fcinfo.flinfo = std::ptr::null_mut();
    fcinfo.context = std::ptr::null_mut();
    fcinfo.resultinfo = std::ptr::null_mut();
    fcinfo.fncollation = pg_sys::InvalidOid;
    fcinfo.isnull = false;
    fcinfo.nargs = args.len() as _;

    for (i, arg) in args.into_iter().enumerate() {
        fcinfo.argnull[i] = arg.is_none();
        fcinfo.arg[i] = arg.unwrap_or(pg_sys::Datum::from(0));
    }

    let result = func(fcinfo_ptr);
    let result = if fcinfo.isnull { None } else { Some(result) };

    pg_sys::pfree(fcinfo_ptr.cast());
    return result;
}

#[cfg(not(feature = "pg11"))]
unsafe fn direct_function_call_as_datum_internal(
    func: impl FnOnce(pg_sys::FunctionCallInfo) -> pg_sys::Datum,
    args: Vec<Option<pg_sys::Datum>>,
) -> Option<pg_sys::Datum> {
    let nargs = args.len();
    let fcinfo_ptr = pg_sys::palloc(
        std::mem::size_of::<pg_sys::FunctionCallInfoBaseData>()
            + std::mem::size_of::<pg_sys::NullableDatum>() * nargs,
    )
    .cast::<pg_sys::FunctionCallInfoBaseData>();

    let fcinfo = fcinfo_ptr.as_mut().unwrap_unchecked();
    fcinfo.flinfo = std::ptr::null_mut();
    fcinfo.context = std::ptr::null_mut();
    fcinfo.resultinfo = std::ptr::null_mut();
    fcinfo.fncollation = pg_sys::InvalidOid;
    fcinfo.isnull = false;
    fcinfo.nargs = nargs as _;

    let arg_slice = fcinfo.args.as_mut_slice(nargs);
    for (i, arg) in args.into_iter().enumerate() {
        arg_slice[i].isnull = arg.is_none();
        arg_slice[i].value = arg.unwrap_or(pg_sys::Datum::from(0));
    }

    let result = func(fcinfo_ptr);
    let result = if fcinfo.isnull { None } else { Some(result) };

    pg_sys::pfree(fcinfo_ptr.cast());
    return result;
}

/// Same as [direct_pg_extern_function_call] but instead returns the direct `Option<pg_sys::Datum>` instead
/// of converting it to a value
///
/// ## Safety
///
/// This function is unsafe as the function you're calling is also unsafe
pub unsafe fn direct_pg_extern_function_call_as_datum(
    func: unsafe extern "C" fn(pg_sys::FunctionCallInfo) -> pg_sys::Datum,
    args: Vec<Option<pg_sys::Datum>>,
) -> Option<pg_sys::Datum> {
    direct_function_call_as_datum_internal(|fcinfo| func(fcinfo), args)
}

#[inline]
pub unsafe fn srf_is_first_call(fcinfo: pg_sys::FunctionCallInfo) -> bool {
    let fcinfo = PgBox::from_pg(fcinfo);
    let flinfo = PgBox::from_pg(fcinfo.flinfo);

    flinfo.fn_extra.is_null()
}

#[inline]
pub unsafe fn srf_first_call_init(
    fcinfo: pg_sys::FunctionCallInfo,
) -> PgBox<pg_sys::FuncCallContext> {
    let funcctx = pg_sys::init_MultiFuncCall(fcinfo);
    PgBox::from_pg(funcctx)
}

#[inline]
pub unsafe fn srf_per_call_setup(
    fcinfo: pg_sys::FunctionCallInfo,
) -> PgBox<pg_sys::FuncCallContext> {
    let funcctx = pg_sys::per_MultiFuncCall(fcinfo);
    PgBox::from_pg(funcctx)
}

#[inline]
pub unsafe fn srf_return_next(
    fcinfo: pg_sys::FunctionCallInfo,
    funcctx: &mut PgBox<pg_sys::FuncCallContext>,
) {
    funcctx.call_cntr += 1;

    let fcinfo = PgBox::from_pg(fcinfo);
    let mut rsi = PgBox::from_pg(fcinfo.resultinfo as *mut pg_sys::ReturnSetInfo);
    rsi.isDone = pg_sys::ExprDoneCond_ExprMultipleResult;
}

#[inline]
pub unsafe fn srf_return_done(
    fcinfo: pg_sys::FunctionCallInfo,
    funcctx: &mut PgBox<pg_sys::FuncCallContext>,
) {
    pg_sys::end_MultiFuncCall(fcinfo, funcctx.as_ptr());
    let fcinfo = PgBox::from_pg(fcinfo);
    let mut rsi = PgBox::from_pg(fcinfo.resultinfo as *mut pg_sys::ReturnSetInfo);
    rsi.isDone = pg_sys::ExprDoneCond_ExprEndResult;
}
