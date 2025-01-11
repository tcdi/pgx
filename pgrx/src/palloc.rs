use crate::memcx::MemCx;
use core::ffi::{self, CStr};
use core::marker::PhantomData;
use core::mem;
use core::ptr::NonNull;

/// A CStr in a palloc
///
/// ABI-compatible with `*const ffi::c_char`
#[repr(transparent)]
pub struct PStr<'mcx>(NonNull<ffi::c_char>, PhantomData<&'mcx MemCx<'mcx>>);

impl<'mcx> PStr<'mcx> {
    /// AKA [`mem::transmute`].
    ///
    /// # Safety
    ///
    /// By invoking this, you assert
    /// - The pointee is data allocated via a Postgres memory context.
    /// - `'this` does not outlive that memory context.
    /// - The constraints of [`CStr::from_ptr`] apply, with a `\0`-terminated pointee
    ///
    /// You are giving this a lifetime bounded by the called-with lifetime. There is no check to
    /// validate your assertion, nor is there a check to validate the pointee is `\0`-terminated.
    #[inline]
    pub(crate) unsafe fn assume_ptr_lives_for<'this>(ptr: NonNull<ffi::c_char>) -> PStr<'this> {
        // SAFETY: The caller is allowed to assign a lifetime to this fn, making it a kind of
        // "lifetime transmutation", regardless of whether we transmute or use struct init.
        unsafe { mem::transmute(ptr) }
    }

    /// Safely introduces a lifetime
    ///
    /// # Safety
    ///
    /// By calling this function, you assert:
    /// - The pointee is data allocated via the referenced `MemCx<'mcx>`.
    /// - The constraints of [`CStr::from_ptr`] apply, with a `\0`-terminated pointee
    #[inline]
    pub unsafe fn assume_ptr_in(ptr: NonNull<ffi::c_char>, _memcx: &MemCx<'mcx>) -> PStr<'mcx> {
        PStr::assume_ptr_lives_for::<'mcx>(ptr)
    }
}

/// A type allocated in a memory context.
pub struct Palloc<'mcx, T>(T, PhantomData<&'mcx MemCx<'mcx>>);
