// ultramodern::renderer::RendererContext backed by fast3d-rs (rust/recomp-render).
//
// Built into the game when RecompRustRuntime.cmake's recomp_add_fast3d_renderer is used.
// The game's own create_render_context decides when to use it (see zelda64recomp.patch:
// RECOMP_RENDERER=fast3d in the environment).

#include "fast3d_render_context.hpp"

#include <cstdio>
#include <cstdlib>
#include <cstring>

#include <SDL.h>
#include <SDL_syswm.h>
// Xlib macros from SDL_syswm.h that collide with names in ultramodern's headers.
#undef None
#undef Status
#undef Success
#undef Always
#undef False
#undef True
#undef LockMask

#include "recomp_render.h"

static_assert(sizeof(RecompRenderViRegs) == sizeof(ultramodern::renderer::ViRegs),
              "RecompRenderViRegs must match ultramodern::renderer::ViRegs");

namespace {

class Fast3dContext final : public ultramodern::renderer::RendererContext {
public:
    explicit Fast3dContext(ultramodern::renderer::WindowHandle window_handle) : window(window_handle) {
        setup_result = ultramodern::renderer::SetupResult::GraphicsDeviceNotFound;
        chosen_api = ultramodern::renderer::GraphicsApi::Vulkan;

        SDL_SysWMinfo info;
        SDL_VERSION(&info.version);
        if (!SDL_GetWindowWMInfo(window, &info)) {
            fprintf(stderr, "[fast3d] SDL_GetWindowWMInfo failed: %s\n", SDL_GetError());
            return;
        }

        uint32_t system = 0;
        void* display = nullptr;
        uint64_t native_window = 0;
        switch (info.subsystem) {
#if defined(SDL_VIDEO_DRIVER_X11)
            case SDL_SYSWM_X11:
                system = RECOMP_RENDER_WINDOW_XLIB;
                display = info.info.x11.display;
                native_window = static_cast<uint64_t>(info.info.x11.window);
                break;
#endif
#if defined(SDL_VIDEO_DRIVER_WAYLAND)
            case SDL_SYSWM_WAYLAND:
                system = RECOMP_RENDER_WINDOW_WAYLAND;
                display = info.info.wl.display;
                native_window = reinterpret_cast<uintptr_t>(info.info.wl.surface);
                break;
#endif
            default:
                fprintf(stderr, "[fast3d] unsupported SDL window system %d\n", static_cast<int>(info.subsystem));
                return;
        }

        int width = 0;
        int height = 0;
        SDL_GetWindowSizeInPixels(window, &width, &height);
        created = recomp_render_fast3d_create(system, display, native_window, width, height);
        if (created) {
            setup_result = ultramodern::renderer::SetupResult::Success;
        }
    }

    ~Fast3dContext() override {
        shutdown();
    }

    bool valid() override {
        return created;
    }

    bool update_config(const ultramodern::renderer::GraphicsConfig& old_config,
                       const ultramodern::renderer::GraphicsConfig& new_config) override {
        // Graphics options (resolution, MSAA, aspect ratio) aren't supported yet.
        return old_config != new_config;
    }

    void enable_instant_present() override {}

    void send_dl(const OSTask* task) override {
        RecompRenderGfxTask gfx{
            task->t.ucode,
            task->t.ucode_size,
            task->t.ucode_data,
            task->t.ucode_data_size,
            task->t.data_ptr,
            task->t.data_size,
        };
        recomp_render_fast3d_send_dl(gfx);
    }

    void update_screen() override {
        RecompRenderViRegs vi;
        std::memcpy(&vi, ultramodern::renderer::get_vi_regs(), sizeof(vi));
        int width = 0;
        int height = 0;
        SDL_GetWindowSizeInPixels(window, &width, &height);
        recomp_render_fast3d_update_screen(vi, width, height);
    }

    void shutdown() override {
        if (created) {
            recomp_render_fast3d_shutdown();
            created = false;
        }
    }

    uint32_t get_display_framerate() const override {
        return 60;
    }

    float get_resolution_scale() const override {
        return 1.0f;
    }

private:
    SDL_Window* window;
    bool created = false;
};

} // namespace

bool recomp_render::fast3d_requested() {
    const char* renderer = std::getenv("RECOMP_RENDERER");
    return renderer != nullptr && std::strcmp(renderer, "fast3d") == 0;
}

std::unique_ptr<ultramodern::renderer::RendererContext> recomp_render::create_fast3d_context(
    ultramodern::renderer::WindowHandle window_handle) {
    return std::make_unique<Fast3dContext>(window_handle);
}
