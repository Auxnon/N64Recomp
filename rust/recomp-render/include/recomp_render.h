/* C interface to rust/recomp-render: fast3d-rs as a recompiled game's renderer.
 *
 * Every function must be called from the same thread (ultramodern's graphics thread),
 * starting with recomp_render_fast3d_create. RDRAM must already be registered with
 * recomp_abi_install_guest_memory. */
#ifndef RECOMP_RENDER_H
#define RECOMP_RENDER_H

#include <stdbool.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Window systems for recomp_render_fast3d_create. */
#define RECOMP_RENDER_WINDOW_XLIB 1    /* display: Display*, window: Window */
#define RECOMP_RENDER_WINDOW_WAYLAND 2 /* display: wl_display*, window: wl_surface* */
#define RECOMP_RENDER_WINDOW_XCB 3     /* display: xcb_connection_t*, window: xcb_window_t */

/* The graphics fields of an OSTask, as guest addresses. */
typedef struct {
    uint32_t ucode;
    uint32_t ucode_size;
    uint32_t ucode_data;
    uint32_t ucode_data_size;
    uint32_t data_ptr;
    uint32_t data_size;
} RecompRenderGfxTask;

/* ultramodern::renderer::ViRegs, field for field. */
typedef struct {
    uint32_t status;
    uint32_t origin;
    uint32_t width;
    uint32_t intr;
    uint32_t v_current_line;
    uint32_t timing;
    uint32_t v_sync;
    uint32_t h_sync;
    uint32_t leap;
    uint32_t h_start;
    uint32_t v_start;
    uint32_t v_burst;
    uint32_t x_scale;
    uint32_t y_scale;
} RecompRenderViRegs;

/* Creates the renderer for the window. The handles must stay valid until shutdown.
 * Returns false (and prints why) on failure. */
bool recomp_render_fast3d_create(uint32_t window_system, void* display, uint64_t window,
                                 uint32_t width, uint32_t height);
void recomp_render_fast3d_send_dl(RecompRenderGfxTask task);
/* width and height are the window's size in pixels. */
void recomp_render_fast3d_update_screen(RecompRenderViRegs vi, uint32_t width, uint32_t height);
void recomp_render_fast3d_shutdown(void);

#ifdef __cplusplus
}
#endif

#endif
