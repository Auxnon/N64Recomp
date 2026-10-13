// Two copies of librecomp's save buffer (pi.cpp's save_read/save_write/save_clear, minus the
// file thread): one behind the reference copies' C++ names, one behind the C entry points
// recomp-runtime calls (recomp_save_*). Each logs its osSendMesg calls. Tests compare them.

#include <cstring>
#include <cstdio>
#include <vector>
#include "save_prelude.hpp"
// The rename is for the reference copies; Rust calls the real name.
#undef osSendMesg

namespace {
    constexpr size_t buffer_size = 0x20000;
    struct Backend {
        std::vector<char> buffer = std::vector<char>(buffer_size);
        std::vector<int32_t> messages; // mq, msg, flag per call
    };
    Backend reference;
    Backend rust;
    uint32_t save_type = (uint32_t)recomp::SaveType::AllowAll;

    void write(Backend& b, uint8_t* rdram, int32_t address, uint32_t offset, uint32_t count) {
        for (gpr i = 0; i < count; i++) {
            b.buffer[offset + i] = MEM_B(i, address);
        }
    }
    void read(Backend& b, uint8_t* rdram, int32_t address, uint32_t offset, uint32_t count) {
        for (gpr i = 0; i < count; i++) {
            MEM_B(i, address) = b.buffer[offset + i];
        }
    }
    s32 send(Backend& b, int32_t mq, int32_t msg, s32 flag) {
        b.messages.insert(b.messages.end(), { mq, msg, flag });
        return 0;
    }
}

recomp::SaveType recomp::get_save_type() { return (recomp::SaveType)save_type; }
bool recomp::eeprom_allowed() {
    auto t = get_save_type();
    return t == SaveType::Eep4k || t == SaveType::Eep16k || t == SaveType::AllowAll;
}
bool recomp::flashram_allowed() {
    auto t = get_save_type();
    return t == SaveType::Flashram || t == SaveType::AllowAll;
}
void ultramodern::error_handling::message_box(const char* message) {
    fprintf(stderr, "%s\n", message);
}

void save_write_ptr(const void* in, uint32_t offset, uint32_t count) {
    memcpy(&reference.buffer[offset], in, count);
}
void save_write(uint8_t* rdram, int32_t address, uint32_t offset, uint32_t count) {
    write(reference, rdram, address, offset, count);
}
void save_read(uint8_t* rdram, int32_t address, uint32_t offset, uint32_t count) {
    read(reference, rdram, address, offset, count);
}
void save_clear(uint32_t start, uint32_t size, char value) {
    std::fill_n(reference.buffer.begin() + start, size, value);
}
s32 ref_osSendMesg(uint8_t*, int32_t mq, int32_t msg, s32 flag) { return send(reference, mq, msg, flag); }

extern "C" {
    void recomp_save_read(uint8_t* rdram, int32_t address, uint32_t offset, uint32_t count) {
        read(rust, rdram, address, offset, count);
    }
    void recomp_save_write(uint8_t* rdram, int32_t address, uint32_t offset, uint32_t count) {
        write(rust, rdram, address, offset, count);
    }
    void recomp_save_write_ptr(const uint8_t* in, uint32_t offset, uint32_t count) {
        memcpy(&rust.buffer[offset], in, count);
    }
    void recomp_save_clear(uint32_t start, uint32_t size, uint8_t value) {
        std::fill_n(rust.buffer.begin() + start, size, (char)value);
    }
    uint32_t recomp_save_type() { return save_type; }
    bool recomp_eeprom_allowed() { return recomp::eeprom_allowed(); }
    bool recomp_flashram_allowed() { return recomp::flashram_allowed(); }
    int32_t recomp_flash_handle() { return recomp::flash_handle; }
    [[noreturn]] void recomp_fatal_error(const char* message) {
        fprintf(stderr, "%s\n", message);
        std::abort();
    }
    s32 osSendMesg(uint8_t*, int32_t mq, int32_t msg, s32 flag) { return send(rust, mq, msg, flag); }

    // Test controls.
    void ctest_set_save_type(uint32_t type) { save_type = type; }
    // Copies out a backend's buffer (0x20000 bytes) and message log; returns the log length.
    size_t ctest_backend(bool use_rust, uint8_t* buffer_out, int32_t* messages_out, size_t max_messages) {
        Backend& b = use_rust ? rust : reference;
        memcpy(buffer_out, b.buffer.data(), buffer_size);
        size_t n = b.messages.size() < max_messages ? b.messages.size() : max_messages;
        memcpy(messages_out, b.messages.data(), n * sizeof(int32_t));
        return b.messages.size();
    }
}
