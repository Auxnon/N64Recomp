//! Bindings to `c/runtime_reference.cpp`: N64ModernRuntime's C++ for the functions
//! recomp-runtime replaces, renamed `ref_*`. Test-only: this crate's unsafe code is outside
//! the port's unsafe budget.

use recomp_abi::RecompContext;

/// A runtime function's C signature, `void f(uint8_t* rdram, recomp_context* ctx)`.
pub type RuntimeFn = unsafe extern "C" fn(*mut u8, *mut RecompContext);

macro_rules! references {
    ($($name:ident)*) => {
        unsafe extern "C" {
            $(pub fn $name(rdram: *mut u8, ctx: *mut RecompContext);)*
        }
    };
}

references! {
    ref___udivdi3 ref___divdi3 ref___umoddi3 ref___ull_div ref___ll_div ref___ll_mul
    ref___ull_rem ref___ull_to_d ref___ull_to_f ref___ull_rshift ref___ll_to_f ref___f_to_ll
    ref___ll_lshift
    ref___checkHardware_msp ref___checkHardware_kmc ref___checkHardware_isv
    ref_is_proutSyncPrintf ref_osVoiceInit
}
