//! Crossing between recompiled C code and Rust.
//!
//! Recompiled code calls runtime functions as `void f(uint8_t* rdram, recomp_context* ctx)`.
//! [`recomp_export!`](crate::recomp_export) defines such a function with a safe Rust body;
//! [`GameFn`] calls recompiled code from Rust.

use std::sync::OnceLock;

use crate::{GuestMemory, RecompContext};

/// The signature of every recompiled function (`recomp_func_t` in `recomp.h`).
pub type RecompFunc = unsafe extern "C" fn(rdram: *mut u8, ctx: *mut RecompContext);

static GUEST_MEMORY: OnceLock<GuestMemory<'static>> = OnceLock::new();

/// Registers the runtime's RDRAM so exported functions can reach it. Call once at startup,
/// before any recompiled code runs. Returns `false` if memory was already registered.
pub fn install_guest_memory(memory: GuestMemory<'static>) -> bool {
    GUEST_MEMORY.set(memory).is_ok()
}

/// The registered RDRAM, if [`install_guest_memory`] has run.
pub fn guest_memory() -> Option<&'static GuestMemory<'static>> {
    GUEST_MEMORY.get()
}

/// C entry point for runtimes that allocate RDRAM themselves (N64ModernRuntime's
/// `recomp.cpp` maps it with `mmap`/`VirtualAlloc`).
///
/// # Safety
///
/// Same contract as [`GuestMemory::from_raw_parts`] with a `'static` lifetime: the mapping
/// must stay valid for the rest of the process.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn recomp_abi_install_guest_memory(rdram: *mut u8, len_bytes: usize) -> bool {
    // SAFETY: forwarded from this function's contract.
    install_guest_memory(unsafe { GuestMemory::from_raw_parts(rdram, len_bytes) })
}

/// Turns the arguments of an exported function into safe references. Used by
/// [`recomp_export!`](crate::recomp_export); not meant to be called directly.
///
/// # Safety
///
/// `ctx` must point to a valid `recomp_context` that nothing else accesses until the
/// returned reference is dropped. Recompiled code guarantees this: the context belongs to
/// the calling game thread and is only touched by the function it calls.
#[doc(hidden)]
#[allow(unsafe_code)]
#[track_caller]
pub unsafe fn enter<'a>(
    rdram: *mut u8,
    ctx: *mut RecompContext,
) -> (&'static GuestMemory<'static>, &'a mut RecompContext) {
    let memory = guest_memory().expect("recomp_export called before install_guest_memory");
    assert_eq!(rdram, memory.as_ptr(), "rdram pointer doesn't match the installed guest memory");
    // SAFETY: the caller guarantees `ctx` is valid and unaliased; `as_mut` rejects null.
    let ctx = unsafe { ctx.as_mut() }.expect("null recomp_context");
    (memory, ctx)
}

/// Defines functions that recompiled code can call, with safe bodies.
///
/// ```ignore
/// recomp_abi::recomp_export! {
///     fn osGetMemSize_recomp(mem: &GuestMemory, ctx: &mut RecompContext) {
///         ctx.set_return_u32(8 * 1024 * 1024);
///     }
/// }
/// ```
///
/// Each function becomes `extern "C" fn name(uint8_t* rdram, recomp_context* ctx)` with an
/// unmangled symbol. The generated item allows `unsafe_code` for the symbol attribute and
/// the pointer conversion, so crates using this can still `#![deny(unsafe_code)]`.
#[macro_export]
macro_rules! recomp_export {
    ($(
        $(#[$attr:meta])*
        fn $name:ident($mem:ident : &GuestMemory, $ctx:ident : &mut RecompContext) $body:block
    )*) => {$(
        $(#[$attr])*
        #[allow(unsafe_code, non_snake_case, clippy::not_unsafe_ptr_arg_deref)]
        #[unsafe(no_mangle)]
        pub extern "C" fn $name(rdram: *mut u8, ctx: *mut $crate::RecompContext) {
            fn body($mem: &$crate::GuestMemory<'static>, $ctx: &mut $crate::RecompContext) $body
            // SAFETY: called from recompiled code, which passes its own thread's context.
            let (memory, context) = unsafe { $crate::export::enter(rdram, ctx) };
            body(memory, context)
        }
    )*};
}

/// A recompiled function that Rust can call (a `LOOKUP_FUNC` result, a thread entry point).
#[derive(Clone, Copy, Debug)]
pub struct GameFn(RecompFunc);

impl GameFn {
    /// # Safety
    ///
    /// `func` must be a recompiled function (or a runtime export) that only touches guest
    /// memory through the `rdram` pointer and the context it is given.
    #[allow(unsafe_code)]
    pub unsafe fn new(func: RecompFunc) -> Self {
        Self(func)
    }

    #[allow(unsafe_code)]
    pub fn call(self, memory: &GuestMemory<'_>, ctx: &mut RecompContext) {
        // SAFETY: `new` requires a recompiled function, which accepts any valid rdram view
        // and context; both come from live references here.
        unsafe { (self.0)(memory.as_ptr(), ctx) }
    }
}
