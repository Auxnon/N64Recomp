// fast3d-rs renderer for N64ModernRuntime games (rust/recomp-render). Linux only for now:
// it takes the SDL window that ultramodern::renderer::WindowHandle holds there.
#ifndef FAST3D_RENDER_CONTEXT_HPP
#define FAST3D_RENDER_CONTEXT_HPP

#include <functional>
#include <memory>

#include "ultramodern/renderer_context.hpp"

namespace recomp_render {
    // Whether RECOMP_RENDERER=fast3d is set.
    bool fast3d_requested();

    // A renderer context drawing with fast3d-rs. Check valid() before using it.
    //
    // on_first_screen_update, if set, runs on the graphics thread before the first screen
    // update. A game without RT64's launcher menu can start itself there: by then the VI
    // thread has run and set its placeholder video mode. Starting earlier (from
    // create_render_context) leaves the VI with no mode, and its first update crashes.
    std::unique_ptr<ultramodern::renderer::RendererContext> create_fast3d_context(
        ultramodern::renderer::WindowHandle window_handle,
        std::function<void()> on_first_screen_update = {});
}

#endif
