//! Bindings to `c/runtime_reference.cpp`: N64ModernRuntime's C++ for the functions
//! recomp-runtime replaces, renamed `ref_*`. Test-only: this crate's unsafe code is outside
//! the port's unsafe budget.

use recomp_abi::RecompContext;

pub mod harness;

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
    ref_osEepromProbe ref_osEepromWrite ref_osEepromLongWrite ref_osEepromRead
    ref_osEepromLongRead
    ref_osFlashInit ref_osFlashReadStatus ref_osFlashReadId ref_osFlashClearStatus
    ref_osFlashAllErase ref_osFlashAllEraseThrough ref_osFlashSectorErase
    ref_osFlashSectorEraseThrough ref_osFlashCheckEraseEnd ref_osFlashWriteBuffer
    ref_osFlashWriteArray ref_osFlashReadArray ref_osFlashChange
    ref_osPfsInitPak ref_osPfsFreeBlocks ref_osPfsAllocateFile ref_osPfsDeleteFile
    ref_osPfsFileState ref_osPfsFindFile ref_osPfsReadWriteFile ref_osPfsChecker
    ref_osPfsNumFiles ref_osPfsRepairId
    ref___osPiGetAccess ref___osPiRelAccess ref_osCartRomInit ref_osDriveRomInit
    ref_osCreatePiManager ref_osPiStartDma ref_osEPiStartDma ref_osEPiReadIo ref_osPiGetStatus
}

/// The size of each stand-in save buffer in `c/save_backend.cpp`.
pub const SAVE_BUFFER_SIZE: usize = 0x20000;

unsafe extern "C" {
    /// Sets the save type both backends report (librecomp's `recomp::SaveType` order).
    pub fn ctest_set_save_type(save_type: u32);
    /// Gives the reference and the Rust side the same ROM.
    pub fn ctest_set_rom(data: *const u8, len: usize);
    fn ctest_backend(use_rust: bool, buffer: *mut u8, messages: *mut i32, max: usize) -> usize;
}

/// One stand-in backend's save bytes and `osSendMesg` log (`mq, msg, flag` per call).
pub fn backend(rust: bool) -> (Vec<u8>, Vec<i32>) {
    let mut buffer = vec![0; SAVE_BUFFER_SIZE];
    let mut messages = vec![0; 3 * 4096];
    // SAFETY: the buffers are as large as the C side copies into.
    let n = unsafe {
        ctest_backend(
            rust,
            buffer.as_mut_ptr(),
            messages.as_mut_ptr(),
            messages.len(),
        )
    };
    messages.truncate(n.min(messages.len()));
    (buffer, messages)
}
