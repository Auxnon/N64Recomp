// N64ModernRuntime's PI exports and their helpers, copied verbatim from librecomp/src/pi.cpp
// (and the OSIoMesg/OSPiHandle layouts from ultramodern/include/ultramodern/ultra64.h) at
// N64ModernRuntime df7e820, with only the export names changed (`f_recomp` -> `ref_f`).
// Regenerate rather than edit. They run against save_backend.cpp's reference buffer and ROM.

#include <cassert>
#include <cstdio>
#include <stdexcept>
#include <vector>
#include "save_prelude.hpp"

typedef uint16_t u16;
struct OSPiHandle_s;
struct OSMesgQueue;
struct OSIoMesgHdr {
    // These 3 reversed due to endianness
    u8 status;                 /* Return status */
    u8 pri;                    /* Message priority (High or Normal) */
    u16 type;                  /* Message type */
    PTR(OSMesgQueue) retQueue; /* Return message queue to notify I/O completion */
};

struct OSIoMesg {
    OSIoMesgHdr	hdr;    /* Message header */
    PTR(void) dramAddr;	/* RDRAM buffer address (DMA) */
    u32 devAddr;	    /* Device buffer address (DMA) */
    u32 size;		    /* DMA transfer size in bytes */
    u32 piHandle;	    /* PI device handle */
};

struct OSPiHandle {
    PTR(OSPiHandle_s)    unused;        /* point to next handle on the table */
    // These four members reversed due to endianness
    u8                   relDuration;   /* domain release duration */
    u8                   pageSize;      /* domain page size */
    u8                   latency;       /* domain latency */
    u8                   type;          /* DEVICE_TYPE_BULK for disk */
    // These three members reversed due to endianness
    u16                  padding;       /* struct alignment padding */
    u8                   domain;        /* which domain */
    u8                   pulse;         /* domain pulse width */
    u32                  baseAddress;   /* Domain address */
    u32                  speed;         /* for roms only */
    /* The following are "private" elements" */
    u32                  transferInfo[18];  /* for disk only */
};

namespace recomp {
    constexpr int32_t cart_handle = (int32_t)0x80800000;
    constexpr int32_t drive_handle = (int32_t)(cart_handle + sizeof(OSPiHandle));
    constexpr uint32_t sram_base = 0x08000000;
    constexpr uint32_t rom_base = 0x10000000;
    constexpr uint32_t drive_base = 0x06000000;
    bool sram_allowed();
    void do_rom_read(uint8_t* rdram, gpr ram_address, uint32_t physical_addr, size_t num_bytes);
    void do_rom_pio(uint8_t* rdram, gpr ram_address, uint32_t physical_addr);
}
#define debug_printf(...)

// The reference copy of the ROM; save_backend.cpp fills it.
extern std::vector<uint8_t> rom;

constexpr uint32_t k1_to_phys(uint32_t addr) {
    return addr & 0x1FFFFFFF;
}

constexpr uint32_t phys_to_k1(uint32_t addr) {
    return addr | 0xA0000000;
}

void recomp::do_rom_read(uint8_t* rdram, gpr ram_address, uint32_t physical_addr, size_t num_bytes) {
    // TODO use word copies when possible

    // TODO handle misaligned DMA
    assert((physical_addr & 0x1) == 0 && "Only PI DMA from aligned ROM addresses is currently supported");
    assert((ram_address & 0x7) == 0 && "Only PI DMA to aligned RDRAM addresses is currently supported");
    uint8_t* rom_addr = rom.data() + physical_addr - recomp::rom_base;
    for (size_t i = 0; i < num_bytes; i++) {
        MEM_B(i, ram_address) = *rom_addr;
        rom_addr++;
    }
}

void recomp::do_rom_pio(uint8_t* rdram, gpr ram_address, uint32_t physical_addr) {
    assert((physical_addr & 0x3) == 0 && "PIO not 4-byte aligned in device, currently unsupported");
    assert((ram_address & 0x3) == 0 && "PIO not 4-byte aligned in RDRAM, currently unsupported");
    uint8_t* rom_addr = rom.data() + physical_addr - recomp::rom_base;
    MEM_B(0, ram_address) = *rom_addr++;
    MEM_B(1, ram_address) = *rom_addr++;
    MEM_B(2, ram_address) = *rom_addr++;
    MEM_B(3, ram_address) = *rom_addr++;
}

void do_dma(RDRAM_ARG PTR(OSMesgQueue) mq, gpr rdram_address, uint32_t physical_addr, uint32_t size, uint32_t direction) {
    // TODO asynchronous transfer
    // TODO implement unaligned DMA correctly
    if (direction == 0) {
        if (physical_addr >= recomp::rom_base) {
            // read cart rom
            recomp::do_rom_read(rdram, rdram_address, physical_addr, size);

            // Send a message to the mq to indicate that the transfer completed
            osSendMesg(rdram, mq, 0, OS_MESG_NOBLOCK);
        } else if (physical_addr >= recomp::sram_base) {
            if (!recomp::sram_allowed()) {
                ultramodern::error_handling::message_box("Attempted to use SRAM saving with other save type");
                ULTRAMODERN_QUICK_EXIT();
            }
            // read sram
            save_read(rdram, rdram_address, physical_addr - recomp::sram_base, size);

            // Send a message to the mq to indicate that the transfer completed
            osSendMesg(rdram, mq, 0, OS_MESG_NOBLOCK);
        } else {
            fprintf(stderr, "[WARN] PI DMA read from unknown region, phys address 0x%08X\n", physical_addr);
        }
    } else {
        if (physical_addr >= recomp::rom_base) {
            // write cart rom
            throw std::runtime_error("ROM DMA write unimplemented");
        } else if (physical_addr >= recomp::sram_base) {
            if (!recomp::sram_allowed()) {
                ultramodern::error_handling::message_box("Attempted to use SRAM saving with other save type");
                ULTRAMODERN_QUICK_EXIT();
            }
            // write sram
            save_write(rdram, rdram_address, physical_addr - recomp::sram_base, size);

            // Send a message to the mq to indicate that the transfer completed
            osSendMesg(rdram, mq, 0, OS_MESG_NOBLOCK);
        } else {
            fprintf(stderr, "[WARN] PI DMA write to unknown region, phys address 0x%08X\n", physical_addr);
        }
    }
}

extern "C" void ref___osPiGetAccess(uint8_t* rdram, recomp_context* ctx) {
}

extern "C" void ref___osPiRelAccess(uint8_t* rdram, recomp_context* ctx) {
}

extern "C" void ref_osCartRomInit(uint8_t* rdram, recomp_context* ctx) {
    OSPiHandle* handle = TO_PTR(OSPiHandle, recomp::cart_handle);
    handle->type = 0; // cart
    handle->baseAddress = phys_to_k1(recomp::rom_base);
    handle->domain = 0;

    ctx->r2 = (gpr)recomp::cart_handle;
}

extern "C" void ref_osDriveRomInit(uint8_t * rdram, recomp_context * ctx) {
    OSPiHandle* handle = TO_PTR(OSPiHandle, recomp::drive_handle);
    handle->type = 1; // bulk
    handle->baseAddress = phys_to_k1(recomp::drive_base);
    handle->domain = 0;

    ctx->r2 = (gpr)recomp::drive_handle;
}

extern "C" void ref_osCreatePiManager(uint8_t* rdram, recomp_context* ctx) {
    ;
}

extern "C" void ref_osPiStartDma(RDRAM_ARG recomp_context* ctx) {
    uint32_t mb = ctx->r4;
    uint32_t pri = ctx->r5;
    uint32_t direction = ctx->r6;
    uint32_t devAddr = ctx->r7 | recomp::rom_base;
    gpr dramAddr = MEM_W(0x10, ctx->r29);
    uint32_t size = MEM_W(0x14, ctx->r29);
    PTR(OSMesgQueue) mq = MEM_W(0x18, ctx->r29);
    uint32_t physical_addr = k1_to_phys(devAddr);

    debug_printf("[pi] DMA from 0x%08X into 0x%08X of size 0x%08X\n", devAddr, dramAddr, size);

    do_dma(PASS_RDRAM mq, dramAddr, physical_addr, size, direction);

    ctx->r2 = 0;
}

extern "C" void ref_osEPiStartDma(RDRAM_ARG recomp_context* ctx) {
    OSPiHandle* handle = TO_PTR(OSPiHandle, ctx->r4);
    OSIoMesg* mb = TO_PTR(OSIoMesg, ctx->r5);
    uint32_t direction = ctx->r6;
    uint32_t devAddr = handle->baseAddress | mb->devAddr;
    gpr dramAddr = mb->dramAddr;
    uint32_t size = mb->size;
    PTR(OSMesgQueue) mq = mb->hdr.retQueue;
    uint32_t physical_addr = k1_to_phys(devAddr);

    debug_printf("[pi] DMA from 0x%08X into 0x%08X of size 0x%08X\n", devAddr, dramAddr, size);

    do_dma(PASS_RDRAM mq, dramAddr, physical_addr, size, direction);

    ctx->r2 = 0;
}

extern "C" void ref_osEPiReadIo(RDRAM_ARG recomp_context * ctx) {
    OSPiHandle* handle = TO_PTR(OSPiHandle, ctx->r4);
    uint32_t devAddr = handle->baseAddress | ctx->r5;
    gpr dramAddr = ctx->r6;
    uint32_t physical_addr = k1_to_phys(devAddr);

    if (physical_addr > recomp::rom_base) {
        // cart rom
        recomp::do_rom_pio(PASS_RDRAM dramAddr, physical_addr);
    } else {
        // sram
        assert(false && "SRAM ReadIo unimplemented");
    }

    ctx->r2 = 0;
}

extern "C" void ref_osPiGetStatus(RDRAM_ARG recomp_context * ctx) {
    ctx->r2 = 0;
}

// Stand-ins for the PI parts of librecomp the Rust calls, built from the same constants.
std::vector<uint8_t> rom;
static std::vector<uint8_t> rust_rom;

extern "C" {
    int32_t recomp_cart_handle() { return recomp::cart_handle; }
    int32_t recomp_drive_handle() { return recomp::drive_handle; }
    const uint8_t* recomp_rom(size_t* len) {
        *len = rust_rom.size();
        return rust_rom.data();
    }
    // Gives both sides the same ROM.
    void ctest_set_rom(const uint8_t* data, size_t len) {
        rom.assign(data, data + len);
        rust_rom.assign(data, data + len);
    }
}
