// N64ModernRuntime's save commands, copied verbatim from librecomp/src/{eep,flash,pak}.cpp
// at N64ModernRuntime df7e820 with only the names changed (`f_recomp` -> `ref_f`). Regenerate
// rather than edit. They run against save_backend.cpp's reference buffer.

#include <array>
#include <cassert>
#include <cstddef>
#include <cstdint>
#include "save_prelude.hpp"

constexpr int eeprom_block_size = 8;
constexpr uint32_t flash_size = 1024 * 1024 / 8; // 1Mbit
constexpr uint32_t page_size = 128;
constexpr uint32_t pages_per_sector = 128;
constexpr uint32_t page_count = flash_size / page_size;

static std::array<char, page_size> write_buffer;

extern "C" void ref_osEepromProbe(uint8_t* rdram, recomp_context* ctx) {
    switch (recomp::get_save_type()) {
        case recomp::SaveType::AllowAll:
        case recomp::SaveType::Eep16k:
            ctx->r2 = 0x02; // EEPROM_TYPE_16K
            break;
        case recomp::SaveType::Eep4k:
            ctx->r2 = 0x01; // EEPROM_TYPE_4K
            break;
        default:
            ctx->r2 = 0x00;
            break;
    }
}

extern "C" void ref_osEepromWrite(uint8_t* rdram, recomp_context* ctx) {
    if (!recomp::eeprom_allowed()) {
        ultramodern::error_handling::message_box("Attempted to use EEPROM saving with other save type");
        ULTRAMODERN_QUICK_EXIT();
    }

    uint8_t eep_address = ctx->r5;
    gpr buffer = ctx->r6;
    int32_t nbytes = eeprom_block_size;

    save_write(rdram, buffer, eep_address * eeprom_block_size, nbytes);

    ctx->r2 = 0;
}

extern "C" void ref_osEepromLongWrite(uint8_t* rdram, recomp_context* ctx) {
    if (!recomp::eeprom_allowed()) {
        ultramodern::error_handling::message_box("Attempted to use EEPROM saving with other save type");
        ULTRAMODERN_QUICK_EXIT();
    }

    uint8_t eep_address = ctx->r5;
    gpr buffer = ctx->r6;
    int32_t nbytes = ctx->r7;

    assert((nbytes % eeprom_block_size) == 0);

    save_write(rdram, buffer, eep_address * eeprom_block_size, nbytes);

    ctx->r2 = 0;
}

extern "C" void ref_osEepromRead(uint8_t* rdram, recomp_context* ctx) {
    if (!recomp::eeprom_allowed()) {
        ultramodern::error_handling::message_box("Attempted to use EEPROM saving with other save type");
        ULTRAMODERN_QUICK_EXIT();
    }

    uint8_t eep_address = ctx->r5;
    gpr buffer = ctx->r6;
    int32_t nbytes = eeprom_block_size;

    save_read(rdram, buffer, eep_address * eeprom_block_size, nbytes);

    ctx->r2 = 0;
}

extern "C" void ref_osEepromLongRead(uint8_t* rdram, recomp_context* ctx) {
    if (!recomp::eeprom_allowed()) {
        ultramodern::error_handling::message_box("Attempted to use EEPROM saving with other save type");
        ULTRAMODERN_QUICK_EXIT();
    }

    uint8_t eep_address = ctx->r5;
    gpr buffer = ctx->r6;
    int32_t nbytes = ctx->r7;

    assert((nbytes % eeprom_block_size) == 0);

    save_read(rdram, buffer, eep_address * eeprom_block_size, nbytes);

    ctx->r2 = 0;
}

extern "C" void ref_osFlashInit(uint8_t * rdram, recomp_context * ctx) {
    if (!recomp::flashram_allowed()) {
        ultramodern::error_handling::message_box("Attempted to use FlashRAM saving with other save type");
        ULTRAMODERN_QUICK_EXIT();
    }

    ctx->r2 = recomp::flash_handle;
}

extern "C" void ref_osFlashReadStatus(uint8_t * rdram, recomp_context * ctx) {
    if (!recomp::flashram_allowed()) {
        ultramodern::error_handling::message_box("Attempted to use FlashRAM saving with other save type");
        ULTRAMODERN_QUICK_EXIT();
    }

    PTR(u8) flash_status = ctx->r4;

    MEM_B(0, flash_status) = 0;
}

extern "C" void ref_osFlashReadId(uint8_t * rdram, recomp_context * ctx) {
    if (!recomp::flashram_allowed()) {
        ultramodern::error_handling::message_box("Attempted to use FlashRAM saving with other save type");
        ULTRAMODERN_QUICK_EXIT();
    }

    PTR(u32) flash_type = ctx->r4;
    PTR(u32) flash_maker = ctx->r5;

    // Mimic a real flash chip's type and maker, as some games actually check if one is present.
    MEM_W(0, flash_type) = 0x11118001;
    MEM_W(0, flash_maker) = 0x00C2001E;
}

extern "C" void ref_osFlashClearStatus(uint8_t * rdram, recomp_context * ctx) {
    if (!recomp::flashram_allowed()) {
        ultramodern::error_handling::message_box("Attempted to use FlashRAM saving with other save type");
        ULTRAMODERN_QUICK_EXIT();
    }

    // Nothing to do here.
}

extern "C" void ref_osFlashAllErase(uint8_t * rdram, recomp_context * ctx) {
    if (!recomp::flashram_allowed()) {
        ultramodern::error_handling::message_box("Attempted to use FlashRAM saving with other save type");
        ULTRAMODERN_QUICK_EXIT();
    }

    save_clear(0, ultramodern::save_size, 0xFF);

    ctx->r2 = 0;
}

extern "C" void ref_osFlashAllEraseThrough(uint8_t * rdram, recomp_context * ctx) {
    if (!recomp::flashram_allowed()) {
        ultramodern::error_handling::message_box("Attempted to use FlashRAM saving with other save type");
        ULTRAMODERN_QUICK_EXIT();
    }

    save_clear(0, ultramodern::save_size, 0xFF);

    ctx->r2 = 0;
}

extern "C" void ref_osFlashSectorErase(uint8_t * rdram, recomp_context * ctx) {
    if (!recomp::flashram_allowed()) {
        ultramodern::error_handling::message_box("Attempted to use FlashRAM saving with other save type");
        ULTRAMODERN_QUICK_EXIT();
    }

    uint32_t page_num = (uint32_t)ctx->r4;

    // Prevent out of bounds erase
    if (page_num >= page_count) {
        ctx->r2 = -1;
        return;
    }

    save_clear(page_num * page_size, page_size, 0xFF);

    ctx->r2 = 0;
}

extern "C" void ref_osFlashSectorEraseThrough(uint8_t * rdram, recomp_context * ctx) {
    if (!recomp::flashram_allowed()) {
        ultramodern::error_handling::message_box("Attempted to use FlashRAM saving with other save type");
        ULTRAMODERN_QUICK_EXIT();
    }

    uint32_t page_num = (uint32_t)ctx->r4;

    // Prevent out of bounds erase
    if (page_num >= page_count) {
        ctx->r2 = -1;
        return;
    }

    save_clear(page_num * page_size, page_size, 0xFF);

    ctx->r2 = 0;
}

extern "C" void ref_osFlashCheckEraseEnd(uint8_t * rdram, recomp_context * ctx) {
    if (!recomp::flashram_allowed()) {
        ultramodern::error_handling::message_box("Attempted to use FlashRAM saving with other save type");
        ULTRAMODERN_QUICK_EXIT();
    }

    // All erases are blocking in this implementation, so this should always return OK.
    ctx->r2 = 0; // FLASH_STATUS_ERASE_OK
}

extern "C" void ref_osFlashWriteBuffer(uint8_t * rdram, recomp_context * ctx) {
    if (!recomp::flashram_allowed()) {
        ultramodern::error_handling::message_box("Attempted to use FlashRAM saving with other save type");
        ULTRAMODERN_QUICK_EXIT();
    }

    OSIoMesg* mb = TO_PTR(OSIoMesg, ctx->r4);
    int32_t pri = ctx->r5;
    PTR(void) dramAddr = ctx->r6;
    PTR(OSMesgQueue) mq = ctx->r7;
    
    // Copy the input data into the write buffer
    for (size_t i = 0; i < page_size; i++) {
        write_buffer[i] = MEM_B(i, dramAddr);
    }

    // Send the message indicating write completion
    osSendMesg(PASS_RDRAM mq, 0, OS_MESG_NOBLOCK);

    ctx->r2 = 0;
}

extern "C" void ref_osFlashWriteArray(uint8_t * rdram, recomp_context * ctx) {
    if (!recomp::flashram_allowed()) {
        ultramodern::error_handling::message_box("Attempted to use FlashRAM saving with other save type");
        ULTRAMODERN_QUICK_EXIT();
    }

    uint32_t page_num = ctx->r4;

    // Copy the write buffer into the save file
    save_write_ptr(write_buffer.data(), page_num * page_size, page_size);

    ctx->r2 = 0;
}

extern "C" void ref_osFlashReadArray(uint8_t * rdram, recomp_context * ctx) {
    if (!recomp::flashram_allowed()) {
        ultramodern::error_handling::message_box("Attempted to use FlashRAM saving with other save type");
        ULTRAMODERN_QUICK_EXIT();
    }

    OSIoMesg* mb = TO_PTR(OSIoMesg, ctx->r4);
    int32_t pri = ctx->r5;
    uint32_t page_num = ctx->r6;
    PTR(void) dramAddr = ctx->r7;
    uint32_t n_pages = MEM_W(0x10, ctx->r29);
    PTR(OSMesgQueue) mq = MEM_W(0x14, ctx->r29);

    uint32_t offset = page_num * page_size;
    uint32_t count = n_pages * page_size;

    // Read from the save file into the provided buffer
    save_read(PASS_RDRAM dramAddr, offset, count);

    // Send the message indicating read completion
    osSendMesg(PASS_RDRAM mq, 0, OS_MESG_NOBLOCK);

    ctx->r2 = 0;
}

extern "C" void ref_osFlashChange(uint8_t * rdram, recomp_context * ctx) {
    if (!recomp::flashram_allowed()) {
        ultramodern::error_handling::message_box("Attempted to use FlashRAM saving with other save type");
        ULTRAMODERN_QUICK_EXIT();
    }

    assert(false);
}

extern "C" void ref_osPfsInitPak(uint8_t * rdram, recomp_context* ctx) {
    ctx->r2 = 1; // PFS_ERR_NOPACK
}

extern "C" void ref_osPfsFreeBlocks(uint8_t * rdram, recomp_context * ctx) {
    ctx->r2 = 1; // PFS_ERR_NOPACK
}

extern "C" void ref_osPfsAllocateFile(uint8_t * rdram, recomp_context * ctx) {
    ctx->r2 = 1; // PFS_ERR_NOPACK
}

extern "C" void ref_osPfsDeleteFile(uint8_t * rdram, recomp_context * ctx) {
    ctx->r2 = 1; // PFS_ERR_NOPACK
}

extern "C" void ref_osPfsFileState(uint8_t * rdram, recomp_context * ctx) {
    ctx->r2 = 1; // PFS_ERR_NOPACK
}

extern "C" void ref_osPfsFindFile(uint8_t * rdram, recomp_context * ctx) {
    ctx->r2 = 1; // PFS_ERR_NOPACK
}

extern "C" void ref_osPfsReadWriteFile(uint8_t * rdram, recomp_context * ctx) {
    ctx->r2 = 1; // PFS_ERR_NOPACK
}

extern "C" void ref_osPfsChecker(uint8_t * rdram, recomp_context * ctx) {
    ctx->r2 = 1; // PFS_ERR_NOPACK
}

extern "C" void ref_osPfsNumFiles(uint8_t * rdram, recomp_context * ctx) {
    s32* max_files = _arg<1, s32*>(rdram, ctx);
    s32* files_used = _arg<2, s32*>(rdram, ctx);

    *max_files = 0;
    *files_used = 0;

    _return<s32>(ctx, 1); // PFS_ERR_NOPACK
}

extern "C" void ref_osPfsRepairId(uint8_t * rdram, recomp_context * ctx) {
    _return<s32>(ctx, 1); // PFS_ERR_NOPACK
}
