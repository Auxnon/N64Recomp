// C reference for the differential tests: thin wrappers over include/recomp.h.
#include <stddef.h>
#include "recomp.h"

void ctest_layout(size_t out[13]) {
    out[0] = sizeof(recomp_context);
    out[1] = _Alignof(recomp_context);
    out[2] = offsetof(recomp_context, r0);
    out[3] = offsetof(recomp_context, r31);
    out[4] = offsetof(recomp_context, f0);
    out[5] = offsetof(recomp_context, f31);
    out[6] = offsetof(recomp_context, hi);
    out[7] = offsetof(recomp_context, lo);
    out[8] = offsetof(recomp_context, f_odd);
    out[9] = offsetof(recomp_context, status_reg);
    out[10] = offsetof(recomp_context, mips3_float_mode);
    out[11] = sizeof(fpr);
    out[12] = offsetof(fpr, fh);
}

uint32_t ctest_read_w(uint8_t* rdram, gpr addr) { return MEM_W(0, addr); }
uint16_t ctest_read_hu(uint8_t* rdram, gpr addr) { return MEM_HU(0, addr); }
int16_t ctest_read_h(uint8_t* rdram, gpr addr) { return MEM_H(0, addr); }
uint8_t ctest_read_bu(uint8_t* rdram, gpr addr) { return MEM_BU(0, addr); }
int8_t ctest_read_b(uint8_t* rdram, gpr addr) { return MEM_B(0, addr); }
uint64_t ctest_read_d(uint8_t* rdram, gpr addr) { return LD(0, addr); }

void ctest_write_w(uint8_t* rdram, gpr addr, uint32_t v) { MEM_W(0, addr) = v; }
void ctest_write_h(uint8_t* rdram, gpr addr, uint16_t v) { MEM_H(0, addr) = v; }
void ctest_write_b(uint8_t* rdram, gpr addr, uint8_t v) { MEM_B(0, addr) = v; }
void ctest_write_d(uint8_t* rdram, gpr addr, uint64_t v) { SD(v, 0, addr); }

gpr ctest_lwl(uint8_t* rdram, gpr initial, gpr addr) { return do_lwl(rdram, initial, 0, addr); }
gpr ctest_lwr(uint8_t* rdram, gpr initial, gpr addr) { return do_lwr(rdram, initial, 0, addr); }
gpr ctest_ldl(uint8_t* rdram, gpr initial, gpr addr) { return do_ldl(rdram, initial, 0, addr); }
gpr ctest_ldr(uint8_t* rdram, gpr initial, gpr addr) { return do_ldr(rdram, initial, 0, addr); }
void ctest_swl(uint8_t* rdram, gpr addr, gpr v) { do_swl(rdram, 0, addr, v); }
void ctest_swr(uint8_t* rdram, gpr addr, gpr v) { do_swr(rdram, 0, addr, v); }
void ctest_sdl(uint8_t* rdram, gpr addr, gpr v) { do_sdl(rdram, 0, addr, v); }
void ctest_sdr(uint8_t* rdram, gpr addr, gpr v) { do_sdr(rdram, 0, addr, v); }

// A stand-in for recompiled game code: stores $a1 at the address in $a0, then calls a
// runtime function implemented in Rust (test_double_recomp) the way generated code does.
void test_double_recomp(uint8_t* rdram, recomp_context* ctx);

RECOMP_FUNC void ctest_game_func(uint8_t* rdram, recomp_context* ctx) {
    MEM_W(0, ctx->r4) = (uint32_t)ctx->r5;
    test_double_recomp(rdram, ctx);
    ctx->r3 = ADD32(ctx->r2, 1);
}
