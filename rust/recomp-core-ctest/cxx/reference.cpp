// The C++ behavior recomp-core reproduces, built with the same toolchain as the C++ tools.
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <string>
#include <algorithm>
#include <filesystem>
#include <unordered_set>
#include <vector>
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


// std::sort of (key, index) pairs by key only; writes the resulting index order to `out`.
void ref_sort_order_u32(const uint32_t* keys, size_t n, size_t* out) {
    std::vector<size_t> order(n);
    for (size_t i = 0; i < n; i++) {
        order[i] = i;
    }
    std::sort(order.begin(), order.end(), [keys](size_t a, size_t b) { return keys[a] < keys[b]; });
    for (size_t i = 0; i < n; i++) {
        out[i] = order[i];
    }
}

// McIlroy's "killer adversary for quicksort": runs std::sort with a comparator that fixes
// values lazily so the partitioning degrades, and writes out the input that does that.
// Sorting the result drives introsort into its heapsort fallback.
void ref_antiqsort_keys(size_t n, uint32_t* keys_out) {
    std::vector<uint32_t> val(n, (uint32_t)n);
    const uint32_t gas = (uint32_t)n;
    uint32_t nsolid = 0;
    size_t candidate = 0;
    std::vector<size_t> ptr(n);
    for (size_t i = 0; i < n; i++) {
        ptr[i] = i;
    }
    std::sort(ptr.begin(), ptr.end(), [&](size_t x, size_t y) {
        if (val[x] == gas && val[y] == gas) {
            if (x == candidate) {
                val[x] = nsolid++;
            } else {
                val[y] = nsolid++;
            }
        }
        if (val[x] == gas) {
            candidate = x;
        } else if (val[y] == gas) {
            candidate = y;
        }
        return val[x] < val[y];
    });
    for (size_t i = 0; i < n; i++) {
        keys_out[i] = val[i];
    }
}


// std::filesystem::path operations: 0 = filename, 1 = stem, 2 = replace_extension(ext).
size_t ref_path_op(int op, const char* path, const char* ext, char* out, size_t cap) {
    std::filesystem::path p{path};
    std::string result;
    switch (op) {
        case 0: result = p.filename().string(); break;
        case 1: result = p.stem().string(); break;
        default: result = p.replace_extension(ext).string(); break;
    }
    return copy_out(result, out, cap);
}

}

