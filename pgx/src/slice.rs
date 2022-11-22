use crate::prelude::*;
use core::marker::PhantomData;
use core::ptr::{self, NonNull};

/// PallocSlice is between slice and Vec: PallocSlice does not assume the underlying T is valid for all indices
/// and so does not implement the safe trait Index, but does let you call an `unsafe fn get` to do so,
/// and manages its own Drop implementation for the pallocation.
pub struct PallocSlice<'a, T> {
    pallocd: NonNull<[T]>,
    _phantom: PhantomData<&'a T>,
}

impl<'a, T> PallocSlice<'a, T> {
    pub unsafe fn from_raw_parts(ptr: NonNull<T>, len: usize) -> Self {
        PallocSlice {
            pallocd: NonNull::new_unchecked(ptr::slice_from_raw_parts_mut(ptr.as_ptr(), len)),
            _phantom: PhantomData,
        }
    }

    /// # Safety
    /// You must know the underlying type at that index is validly initialized in Rust.
    #[inline]
    pub unsafe fn get(&self, index: usize) -> Option<&T> {
        index
            .le(&self.pallocd.len())
            .then(|| self.pallocd.as_ptr().cast::<T>().add(index).as_ref().unwrap_unchecked())
    }

    /// # Safety
    /// You must know the underlying type at that index is validly initialized in Rust,
    /// AND that the index is inbounds.
    #[inline]
    pub unsafe fn get_unchecked(&self, index: usize) -> &T {
        self.pallocd.as_ptr().cast::<T>().add(index).as_ref().unwrap_unchecked()
    }
}

impl<'a, T> Drop for PallocSlice<'a, T> {
    fn drop(&mut self) {
        unsafe { pg_sys::pfree(self.pallocd.cast().as_ptr()) }
    }
}
