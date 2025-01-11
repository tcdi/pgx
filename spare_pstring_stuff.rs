enum Encoding {
    Bytes,
    Utf8,
}
/// Pallocated, extensible string, that does not assume an encoding.
/// 
/// Like StringInfo, but with less FFI.
/// Note that the 0-len str.
pub struct PString<'mcx> {
    ptr: NonNull<ffi::c_char>,
    mcx: &'mcx MemCx<'mcx>,
    len: u32,
    cap: u32,
}

impl<'mcx> PString<'mcx> {
    pub fn new_in(mcx: &MemCx<'mcx>) -> PString<'mcx> {
        todo!()
    }

    /// Into a pallocated string.
    pub fn into_pstr(self) -> PStr<'mcx> {
        PStr(self.ptr, PhantomData)
    }
}


struct CloneIn();
struct IsRef();
struct IsPalloc();

trait AllocAs {}
impl AllocAs for CloneIn {
}
impl AllocAs for IsRef {
}
impl AllocAs for IsPalloc {
}

pub struct PStrBuilder<M, Alloc> {
    memcx: M,
    raw_ptr: Option<*mut ffi::c_char>,
    alloc: Alloc,
}

/// Instead of a proliferation of "create this from so-and-so"
pub fn build_from(builder: PStrBuilder<MemCx<'mcx>>) -> PString<'mcx> {
    todo!()
}

impl<M> PStrBuilder<M> {

}

impl<'mcx> PStrBuilder<MemCx<'mcx>> {

}