// N64ModernRuntime's C++ for the functions recomp-runtime replaces, copied verbatim from
// librecomp/src/{math_routines,print,ultra_stubs}.cpp at N64ModernRuntime df7e820 with only
// the names changed (`f_recomp` -> `ref_f`), so the Rust ports can be checked against them.
// Regenerate rather than edit.

#include <cstdint>
#include <vector>
#include "recomp.h"

extern "C" void ref___udivdi3(uint8_t * rdram, recomp_context * ctx) {
    uint64_t a = (ctx->r4 << 32) | ((ctx->r5 << 0) & 0xFFFFFFFFu);
    uint64_t b = (ctx->r6 << 32) | ((ctx->r7 << 0) & 0xFFFFFFFFu);
    uint64_t ret = a / b;

    ctx->r2 = (int32_t)(ret >> 32);
    ctx->r3 = (int32_t)(ret >> 0);
}

extern "C" void ref___divdi3(uint8_t * rdram, recomp_context * ctx) {
    int64_t a = (ctx->r4 << 32) | ((ctx->r5 << 0) & 0xFFFFFFFFu);
    int64_t b = (ctx->r6 << 32) | ((ctx->r7 << 0) & 0xFFFFFFFFu);
    int64_t ret = a / b;

    ctx->r2 = (int32_t)(ret >> 32);
    ctx->r3 = (int32_t)(ret >> 0);
}

extern "C" void ref___umoddi3(uint8_t * rdram, recomp_context * ctx) {
    uint64_t a = (ctx->r4 << 32) | ((ctx->r5 << 0) & 0xFFFFFFFFu);
    uint64_t b = (ctx->r6 << 32) | ((ctx->r7 << 0) & 0xFFFFFFFFu);
    uint64_t ret = a % b;

    ctx->r2 = (int32_t)(ret >> 32);
    ctx->r3 = (int32_t)(ret >> 0);
}

extern "C" void ref___ull_div(uint8_t * rdram, recomp_context * ctx) {
    uint64_t a = (ctx->r4 << 32) | ((ctx->r5 << 0) & 0xFFFFFFFFu);
    uint64_t b = (ctx->r6 << 32) | ((ctx->r7 << 0) & 0xFFFFFFFFu);
    uint64_t ret = a / b;

    ctx->r2 = (int32_t)(ret >> 32);
    ctx->r3 = (int32_t)(ret >> 0);
}

extern "C" void ref___ll_div(uint8_t * rdram, recomp_context * ctx) {
    int64_t a = (ctx->r4 << 32) | ((ctx->r5 << 0) & 0xFFFFFFFFu);
    int64_t b = (ctx->r6 << 32) | ((ctx->r7 << 0) & 0xFFFFFFFFu);
    int64_t ret = a / b;

    ctx->r2 = (int32_t)(ret >> 32);
    ctx->r3 = (int32_t)(ret >> 0);
}

extern "C" void ref___ll_mul(uint8_t * rdram, recomp_context * ctx) {
    uint64_t a = (ctx->r4 << 32) | ((ctx->r5 << 0) & 0xFFFFFFFFu);
    uint64_t b = (ctx->r6 << 32) | ((ctx->r7 << 0) & 0xFFFFFFFFu);
    uint64_t ret = a * b;

    ctx->r2 = (int32_t)(ret >> 32);
    ctx->r3 = (int32_t)(ret >> 0);
}

extern "C" void ref___ull_rem(uint8_t * rdram, recomp_context * ctx) {
    uint64_t a = (ctx->r4 << 32) | ((ctx->r5 << 0) & 0xFFFFFFFFu);
    uint64_t b = (ctx->r6 << 32) | ((ctx->r7 << 0) & 0xFFFFFFFFu);
    uint64_t ret = a % b;

    ctx->r2 = (int32_t)(ret >> 32);
    ctx->r3 = (int32_t)(ret >> 0);
}

extern "C" void ref___ull_to_d(uint8_t * rdram, recomp_context * ctx) {
    uint64_t a = (ctx->r4 << 32) | ((ctx->r5 << 0) & 0xFFFFFFFFu);
    double ret = (double)a;

    ctx->f0.d = ret;
}

extern "C" void ref___ull_to_f(uint8_t * rdram, recomp_context * ctx) {
    uint64_t a = (ctx->r4 << 32) | ((ctx->r5 << 0) & 0xFFFFFFFFu);
    float ret = (float)a;

    ctx->f0.fl = ret;
}

extern "C" void ref___ull_rshift(uint8_t * rdram, recomp_context * ctx) {
    uint64_t a = (ctx->r4 << 32) | ((ctx->r5 << 0) & 0xFFFFFFFFu);
    uint64_t b = (ctx->r6 << 32) | ((ctx->r7 << 0) & 0xFFFFFFFFu);
    uint64_t ret = a >> b;

    ctx->r2 = (int32_t)(ret >> 32);
    ctx->r3 = (int32_t)(ret >> 0);
}

extern "C" void ref___ll_to_f(uint8_t * rdram, recomp_context * ctx) {
    int64_t a = (ctx->r4 << 32) | ((ctx->r5 << 0) & 0xFFFFFFFFu);
    float ret = (float)a;

    ctx->f0.fl = ret;
}

extern "C" void ref___f_to_ll(uint8_t * rdram, recomp_context * ctx) {
    int64_t ret = (int64_t)ctx->f12.fl;

    ctx->r2 = (int32_t)(ret >> 32);
    ctx->r3 = (int32_t)(ret >> 0);
}

extern "C" void ref___ll_lshift(uint8_t * rdram, recomp_context * ctx) {
    uint64_t a = (ctx->r4 << 32) | ((ctx->r5 << 0) & 0xFFFFFFFFu);
    uint64_t b = (ctx->r6 << 32) | ((ctx->r7 << 0) & 0xFFFFFFFFu);
    uint64_t ret = a << b;

    ctx->r2 = (int32_t)(ret >> 32);
    ctx->r3 = (int32_t)(ret >> 0);
}

extern "C" void ref___checkHardware_msp(uint8_t * rdram, recomp_context * ctx) {
    ctx->r2 = 0;
}

extern "C" void ref___checkHardware_kmc(uint8_t * rdram, recomp_context * ctx) {
    ctx->r2 = 0;
}

extern "C" void ref___checkHardware_isv(uint8_t * rdram, recomp_context * ctx) {
    ctx->r2 = 0;
}

extern "C" void ref_is_proutSyncPrintf(uint8_t * rdram, recomp_context * ctx) {
    // Buffering to speed up print performance
    static std::vector<char> print_buffer;

    gpr buf = ctx->r5;
    size_t size = ctx->r6;

    //for (size_t i = 0; i < size; i++) {
    //    // Add the new character to the buffer
    //    char cur_char = MEM_B(i, buf);

    //    // If the new character is a newline, flush the buffer
    //    if (cur_char == '\n') {
    //        std::string utf8_str = Encoding::decode_eucjp(std::string_view{ print_buffer.data(), print_buffer.size() });
    //        puts(utf8_str.c_str());
    //        print_buffer.clear();
    //    } else {
    //        print_buffer.push_back(cur_char);
    //    }
    //}

    //fwrite(to_print.get(), size, 1, stdout);

    ctx->r2 = 1;
}

extern "C" void ref_osVoiceInit(uint8_t * rdram, recomp_context * ctx) {
    ctx->r2 = 11; // CONT_ERR_DEVICE
}
