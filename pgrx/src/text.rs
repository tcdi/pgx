#![deny(unsafe_op_in_unsafe_fn)]
use crate::datum::BorrowDatum;
use crate::layout::PassBy;
use crate::pgrx_sql_entity_graph::metadata::{
    ArgumentError, Returns, ReturnsError, SqlMapping, SqlTranslatable,
};
use crate::{pg_sys, varlena};
use alloc::borrow::Cow;
use alloc::string::String;
use core::borrow::Borrow;
use core::ops::{Deref, DerefMut};
use core::{ptr, str};

use bstr::{BStr, ByteSlice};

// We reexport these types so people don't have to care whether they're pulled from BStr or std,
// they just use the ones from pgrx::text::*
pub use bstr::{Bytes, Chars};
pub use core::str::{Utf8Chunks, Utf8Error};

/// A Postgres string, AKA `TEXT`.
///
/// This is a varlena: a reference to a variable-length header followed by a slice of bytes.
#[repr(transparent)]
pub struct Text([u8]);

/// Data field of a TEXT varlena
///
/// Usually this will be UTF-8, but this is not always strictly enforced by PostgreSQL.
#[repr(transparent)]
pub struct TextData([u8]);

impl TextData {
    /// Reborrow `&Text as `&BStr`
    ///
    /// We do not implement Deref to BStr or [u8] because we'd like to expose a more selective API.
    /// Several fn that [u8] implements are implemented very differently on str, and we would like
    /// the API of Text to "feel like" that of str in most cases.
    fn as_bstr(&self) -> &BStr {
        self.as_bytes().borrow()
    }

    /// Obtain a reference to the Text's data as bytes
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Obtain a mutable reference to the Text's data as bytes
    ///
    /// # Safety
    /// Like [`str::as_bytes_mut`], this can cause problems if you change Text in a way that
    /// your database is not specified to support, so the caller must assure that it remains in
    /// a valid encoding for the database.
    pub unsafe fn as_bytes_mut(&mut self) -> &mut [u8] {
        &mut self.0
    }

    /// Iterate over the UTF-8 characters of this Text
    ///
    /// If the data is not UTF-8, the replacement character � is returned.
    pub fn chars(&self) -> Chars<'_> {
        self.as_bstr().chars()
    }

    /// Iterate over the Text's data as bytes
    pub fn bytes(&self) -> Bytes<'_> {
        self.as_bstr().bytes()
    }

    /// Is the data ASCII?
    pub fn is_ascii(&self) -> bool {
        self.as_bytes().is_ascii()
    }

    /// Is this slice nonzero len?
    pub fn is_empty(&self) -> bool {
        self.as_bytes().is_empty()
    }

    /// Length of the data in bytes
    pub fn len(&self) -> usize {
        self.as_bytes().len()
    }

    /// Obtain a reference to the data if it is a UTF-8 str
    pub fn to_str(&self) -> Result<&str, Utf8Error> {
        str::from_utf8(self.as_bytes())
    }

    /// You have two cows. Both are UTF-8 data.
    ///
    /// One is completely UTF-8, but the other is allocated and non-UTF-8 is patched over with �.
    pub fn to_str_lossy(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(self.as_bytes())
    }

    /// Iterate over the UTF-8 chunks of the Text's data
    pub fn utf8_chunks(&self) -> Utf8Chunks {
        self.as_bytes().utf8_chunks()
    }
}

impl Text {
    /// Length of the entire varlena in bytes
    pub fn vl_len(&self) -> usize {
        self.0.len()
    }
}

impl Deref for Text {
    type Target = TextData;
    fn deref(&self) -> &Self::Target {
        let self_ptr = self as *const Text as *const pg_sys::varlena;
        unsafe { &*varlena_to_text_data(self_ptr.cast_mut()) }
    }
}

impl DerefMut for Text {
    fn deref_mut(&mut self) -> &mut Self::Target {
        let self_ptr = self as *mut Text as *mut pg_sys::varlena;
        unsafe { &mut *varlena_to_text_data(self_ptr) }
    }
}

unsafe fn varlena_to_text_data(vptr: *mut pg_sys::varlena) -> *mut TextData {
    unsafe {
        let len = varlena::varsize_any_exhdr(vptr);
        let data = varlena::vardata_any(vptr).cast_mut();

        ptr::slice_from_raw_parts_mut(data.cast::<u8>(), len) as *mut TextData
    }
}

unsafe impl BorrowDatum for Text {
    const PASS: PassBy = PassBy::Ref;
    unsafe fn point_from(ptr: ptr::NonNull<u8>) -> ptr::NonNull<Self> {
        unsafe {
            let len = varlena::varsize_any(ptr.as_ptr().cast());
            ptr::NonNull::new_unchecked(
                ptr::slice_from_raw_parts_mut(ptr.as_ptr(), len) as *mut Text
            )
        }
    }
}

unsafe impl<'dat> SqlTranslatable for &'dat Text {
    fn argument_sql() -> Result<SqlMapping, ArgumentError> {
        Ok(SqlMapping::literal("TEXT"))
    }
    fn return_sql() -> Result<Returns, ReturnsError> {
        Ok(Returns::One(SqlMapping::literal("TEXT")))
    }
}
