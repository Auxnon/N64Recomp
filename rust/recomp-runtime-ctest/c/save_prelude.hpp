// Stand-ins for the N64ModernRuntime declarations librecomp's save commands use, so the
// verbatim copies in save_reference.cpp compile on their own. save_backend.cpp defines them.
#pragma once

#include <cstdint>
#include <cstdlib>
#include <type_traits>
#include "recomp.h"

typedef int32_t s32;
typedef uint8_t u8;
typedef uint32_t u32;
#define PTR(x) int32_t
#define RDRAM_ARG uint8_t *rdram,
#define PASS_RDRAM rdram,
#define TO_PTR(type, var) ((type*)(&rdram[(uint64_t)var - 0xFFFFFFFF80000000]))

struct OSIoMesg;
struct OSMesgQueue;
typedef PTR(void) OSMesg;
#define OS_MESG_NOBLOCK 0

namespace recomp {
    enum class SaveType { None, Eep4k, Eep16k, Sram, Flashram, AllowAll };
    SaveType get_save_type();
    bool eeprom_allowed();
    bool flashram_allowed();
    constexpr int32_t flash_handle = (int32_t)0x80800050;
}

namespace ultramodern {
    constexpr uint32_t save_size = 1024 * 1024 / 8;
    namespace error_handling {
        void message_box(const char* message);
    }
}
#define ULTRAMODERN_QUICK_EXIT() std::abort()

// The reference save buffer (librecomp's pi.cpp functions, on a separate buffer).
void save_write_ptr(const void* in, uint32_t offset, uint32_t count);
void save_write(RDRAM_ARG PTR(void) rdram_address, uint32_t offset, uint32_t count);
void save_read(RDRAM_ARG PTR(void) rdram_address, uint32_t offset, uint32_t count);
void save_clear(uint32_t start, uint32_t size, char value);

// The reference's osSendMesg calls are logged apart from Rust's.
#define osSendMesg ref_osSendMesg
s32 ref_osSendMesg(uint8_t* rdram, PTR(OSMesgQueue) mq, OSMesg msg, s32 flag);

// librecomp/include/librecomp/helpers.hpp, the parts pak.cpp uses.
template<int index, typename T>
T _arg(uint8_t* rdram, recomp_context* ctx) {
    gpr raw_arg = (&ctx->r4)[index];
    return TO_PTR(std::remove_pointer_t<T>, raw_arg);
}

template<typename T>
void _return(recomp_context* ctx, T val) {
    ctx->r2 = int32_t(val);
}
