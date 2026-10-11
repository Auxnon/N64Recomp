// The C++ behavior recomp-core reproduces, built with the same toolchain as the C++ tools.
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <string>
#include <unordered_set>
#include "fmt/format.h"

extern "C" {

// Inserts `keys` in order into a std::unordered_set and writes its iteration order to `out`.
// Returns the set's size.
size_t ref_set_order_u32(const uint32_t* keys, size_t n, uint32_t* out) {
    std::unordered_set<uint32_t> set;
    for (size_t i = 0; i < n; i++) {
        set.insert(keys[i]);
    }
    size_t i = 0;
    for (uint32_t key : set) {
        out[i++] = key;
    }
    return set.size();
}

size_t ref_set_order_i32(const int32_t* keys, size_t n, int32_t* out) {
    std::unordered_set<int32_t> set;
    for (size_t i = 0; i < n; i++) {
        set.insert(keys[i]);
    }
    size_t i = 0;
    for (int32_t key : set) {
        out[i++] = key;
    }
    return set.size();
}

// fmt::format of one integer with the given spec, e.g. "{:#X}". Returns the length written.
static size_t copy_out(const std::string& s, char* out, size_t cap) {
    size_t len = s.size() < cap ? s.size() : cap;
    memcpy(out, s.data(), len);
    return len;
}

size_t ref_fmt_hex_alt_i64(int64_t value, char* out, size_t cap) {
    return copy_out(fmt::format("{:#X}", value), out, cap);
}

size_t ref_fmt_hex_alt_u16(uint16_t value, char* out, size_t cap) {
    return copy_out(fmt::format("{:#X}", value), out, cap);
}

size_t ref_fmt_hex_alt_i16(int16_t value, char* out, size_t cap) {
    return copy_out(fmt::format("{:#X}", value), out, cap);
}

}
