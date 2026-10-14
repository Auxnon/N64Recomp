// librecomp's save buffer and save-file thread (librecomp/src/pi.cpp) and its backup-file
// helpers (librecomp/src/files.cpp), copied verbatim from N64ModernRuntime df7e820 inside namespace
// `refsave`, so recomp-runtime's saving.rs can be checked against them on real files.
// Regenerate rather than edit; the stand-ins and test controls follow the copies.

#include <algorithm>
#include <atomic>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <mutex>
#include <string>
#include <thread>
#include <vector>
#include "recomp.h"
// concurrentqueue.h defines this for the semaphore header.
#define MOODYCAMEL_DELETE_FUNCTION = delete
#include "vendor/lightweightsemaphore.h"

#define RDRAM_ARG uint8_t *rdram,
#define RDRAM_ARG1 uint8_t *rdram
#define PASS_RDRAM rdram,
#define PTR(x) int32_t

namespace refsave {
namespace recomp {
    enum class SaveType { None, Eep4k, Eep16k, Sram, Flashram, AllowAll };
    SaveType get_save_type();
    std::u8string current_game_id();
    std::ifstream open_input_backup_file(const std::filesystem::path& filepath, std::ios_base::openmode mode);
    std::ifstream open_input_file_with_backup(const std::filesystem::path& filepath, std::ios_base::openmode mode);
    std::ofstream open_output_file_with_backup(const std::filesystem::path& filepath, std::ios_base::openmode mode);
    bool finalize_output_file_with_backup(const std::filesystem::path& filepath);
}
namespace ultramodern {
    void init_saving(RDRAM_ARG1);
    void change_save_file(const std::u8string& subfolder, const std::u8string& name);
    std::filesystem::path get_save_file_path();
    void join_saving_thread();
    namespace error_handling { void message_box(const char* message); }
}

// ---- librecomp/src/files.cpp ----

constexpr std::u8string_view backup_suffix = u8".bak";
constexpr std::u8string_view temp_suffix = u8".temp";

std::ifstream recomp::open_input_backup_file(const std::filesystem::path& filepath, std::ios_base::openmode mode) {
    std::filesystem::path backup_path{filepath};
    backup_path += backup_suffix;
    return std::ifstream{backup_path, mode};
}

std::ifstream recomp::open_input_file_with_backup(const std::filesystem::path& filepath, std::ios_base::openmode mode) {
    std::ifstream ret{filepath, mode};

    // Check if the file failed to open and open the corresponding backup file instead if so.
    if (!ret.good()) {
        return open_input_backup_file(filepath, mode);
    }

    return ret;
}

std::ofstream recomp::open_output_file_with_backup(const std::filesystem::path& filepath, std::ios_base::openmode mode) {
    std::filesystem::path temp_path{filepath};
    temp_path += temp_suffix;
    std::ofstream temp_file_out{ temp_path, mode };

    return temp_file_out;
}

bool recomp::finalize_output_file_with_backup(const std::filesystem::path& filepath) {
    std::filesystem::path backup_path{filepath};
    backup_path += backup_suffix;

    std::filesystem::path temp_path{filepath};
    temp_path += temp_suffix;

    std::error_code ec;
    if (std::filesystem::exists(filepath, ec)) {
        std::filesystem::copy_file(filepath, backup_path, std::filesystem::copy_options::overwrite_existing, ec);
        if (ec) {
            return false;
        }
    }
    std::filesystem::copy_file(temp_path, filepath, std::filesystem::copy_options::overwrite_existing, ec);
    if (ec) {
        return false;
    }
    std::filesystem::remove(temp_path, ec);
    return true;
}


// ---- librecomp/src/pi.cpp ----

struct {
    std::vector<char> save_buffer;
    std::thread saving_thread;
    std::filesystem::path save_file_path;
    moodycamel::LightweightSemaphore write_sempahore;
    // Used to tell the saving thread that a file swap is pending.
    moodycamel::LightweightSemaphore swap_file_pending_sempahore;
    // Used to tell the consumer thread that the saving thread is ready for a file swap.
    moodycamel::LightweightSemaphore swap_file_ready_sempahore;
    std::mutex save_buffer_mutex;
} save_context;

const std::u8string save_folder = u8"saves";

extern std::filesystem::path config_path;

std::filesystem::path ultramodern::get_save_file_path() {
    return save_context.save_file_path;
}

void set_save_file_path(const std::u8string& subfolder, const std::u8string& name) {
    std::filesystem::path save_folder_path = config_path / save_folder;
    if (!subfolder.empty()) {
        save_folder_path = save_folder_path / subfolder;
    }
    save_context.save_file_path = save_folder_path / (name + u8".bin");
}

void update_save_file() {
    bool saving_failed = false;
    {
        std::ofstream save_file = recomp::open_output_file_with_backup(ultramodern::get_save_file_path(), std::ios_base::binary);

        if (save_file.good()) {
            std::lock_guard lock{ save_context.save_buffer_mutex };
            save_file.write(save_context.save_buffer.data(), save_context.save_buffer.size());
        }
        else {
            saving_failed = true;
        }
    }
    if (!saving_failed) {
        saving_failed = !recomp::finalize_output_file_with_backup(ultramodern::get_save_file_path());
    }
    if (saving_failed) {
        ultramodern::error_handling::message_box("Failed to write to the save file. Check your file permissions and whether the save folder has been moved to Dropbox or similar, as this can cause issues.");
    }
}

extern std::atomic_bool exited;

void saving_thread_func(RDRAM_ARG1) {
    while (!exited) {
        bool save_buffer_updated = false;
        // Repeatedly wait for a new action to be sent.
        constexpr int64_t wait_time_microseconds = 10000;
        constexpr int max_actions = 128;
        int num_actions = 0;

        // Wait up to the given timeout for a write to come in. Allow multiple writes to coalesce together into a single save.
        // Cap the number of coalesced writes to guarantee that the save buffer eventually gets written out to the file even if the game
        // is constantly sending writes.
        while (save_context.write_sempahore.wait(wait_time_microseconds) && num_actions < max_actions) {
            save_buffer_updated = true;
            num_actions++;
        }

        // If an action came through that affected the save file, save the updated contents.
        if (save_buffer_updated) {
            update_save_file();
        }

        if (save_context.swap_file_pending_sempahore.tryWait()) {
            save_context.swap_file_ready_sempahore.signal();
        }
    }
}

void save_write_ptr(const void* in, uint32_t offset, uint32_t count) {
    assert(offset + count <= save_context.save_buffer.size());

    {
        std::lock_guard lock { save_context.save_buffer_mutex };
        memcpy(&save_context.save_buffer[offset], in, count);
    }
    
    save_context.write_sempahore.signal();
}

void save_write(RDRAM_ARG PTR(void) rdram_address, uint32_t offset, uint32_t count) {
    assert(offset + count <= save_context.save_buffer.size());

    {
        std::lock_guard lock { save_context.save_buffer_mutex };
        for (gpr i = 0; i < count; i++) {
            save_context.save_buffer[offset + i] = MEM_B(i, rdram_address);
        }
    }

    save_context.write_sempahore.signal();
}

void save_read(RDRAM_ARG PTR(void) rdram_address, uint32_t offset, uint32_t count) {
    assert(offset + count <= save_context.save_buffer.size());

    std::lock_guard lock { save_context.save_buffer_mutex };
    for (gpr i = 0; i < count; i++) {
        MEM_B(i, rdram_address) = save_context.save_buffer[offset + i];
    }
}

void save_clear(uint32_t start, uint32_t size, char value) {
    assert(start + size < save_context.save_buffer.size());

    {
        std::lock_guard lock { save_context.save_buffer_mutex };
        std::fill_n(save_context.save_buffer.begin() + start, size, value);
    }

    save_context.write_sempahore.signal();
}

size_t get_save_size(recomp::SaveType save_type) {
    switch (save_type) {
        case recomp::SaveType::AllowAll:
        case recomp::SaveType::Flashram:
            return 0x20000;
        case recomp::SaveType::Sram:
            return 0x8000;
        case recomp::SaveType::Eep16k:
            return 0x800;
        case recomp::SaveType::Eep4k:
            return 0x200;
        case recomp::SaveType::None:
            return 0;
    }
    return 0;
}

void read_save_file() {
    std::filesystem::path save_file_path = ultramodern::get_save_file_path();

    // Ensure the save file directory exists.
    std::filesystem::create_directories(save_file_path.parent_path());

    // Read the save file if it exists.
    std::ifstream save_file = recomp::open_input_file_with_backup(save_file_path, std::ios_base::binary);
    if (save_file.good()) {
        save_file.read(save_context.save_buffer.data(), save_context.save_buffer.size());
    }
    else {
        // Otherwise clear the save file to all zeroes.
        std::fill(save_context.save_buffer.begin(), save_context.save_buffer.end(), 0);
    }
}

void ultramodern::init_saving(RDRAM_ARG1) {
    set_save_file_path(u8"", recomp::current_game_id());

    save_context.save_buffer.resize(get_save_size(recomp::get_save_type()));

    read_save_file();

    save_context.saving_thread = std::thread{saving_thread_func, PASS_RDRAM};
}

void ultramodern::change_save_file(const std::u8string& subfolder, const std::u8string& name) {
    // Tell the saving thread that a file swap is pending.
    save_context.swap_file_pending_sempahore.signal();
    // Wait until the saving thread indicates it's ready to swap files.
    save_context.swap_file_ready_sempahore.wait();
    // Perform the save file swap.
    set_save_file_path(subfolder, name);
    read_save_file();
}

void ultramodern::join_saving_thread() {
    if (save_context.saving_thread.joinable()) {
        save_context.saving_thread.join();
    }
}

// ---- Stand-ins and test controls ----

std::filesystem::path config_path;
std::atomic_bool exited = false;
static recomp::SaveType save_type = recomp::SaveType::Flashram;
static std::u8string game_id;
static int reported = 0;

recomp::SaveType recomp::get_save_type() { return save_type; }
std::u8string recomp::current_game_id() { return game_id; }
void ultramodern::error_handling::message_box(const char*) { reported++; }

static size_t snapshot(uint8_t* out, size_t capacity) {
    std::lock_guard lock{ save_context.save_buffer_mutex };
    size_t n = std::min(capacity, save_context.save_buffer.size());
    memcpy(out, save_context.save_buffer.data(), n);
    return save_context.save_buffer.size();
}
} // namespace refsave

static std::u8string u8(const char* text) {
    return std::u8string{ reinterpret_cast<const char8_t*>(text) };
}

extern "C" {
    void ctest_ref_start(const char* config_dir, const char* game, uint32_t type) {
        refsave::config_path = std::filesystem::path{ u8(config_dir) };
        refsave::game_id = u8(game);
        refsave::save_type = (refsave::recomp::SaveType)type;
        refsave::ultramodern::init_saving(nullptr);
    }
    void ctest_ref_write_ptr(const uint8_t* in, uint32_t offset, uint32_t count) { refsave::save_write_ptr(in, offset, count); }
    void ctest_ref_write(uint8_t* rdram, int32_t address, uint32_t offset, uint32_t count) { refsave::save_write(rdram, address, offset, count); }
    void ctest_ref_read(uint8_t* rdram, int32_t address, uint32_t offset, uint32_t count) { refsave::save_read(rdram, address, offset, count); }
    void ctest_ref_clear(uint32_t start, uint32_t size, uint8_t value) { refsave::save_clear(start, size, (char)value); }
    void ctest_ref_change(const char* subfolder, const char* name) { refsave::ultramodern::change_save_file(u8(subfolder), u8(name)); }
    size_t ctest_ref_path(char* out, size_t capacity) {
        std::string path = refsave::ultramodern::get_save_file_path().string();
        memcpy(out, path.data(), std::min(capacity, path.size()));
        return path.size();
    }
    size_t ctest_ref_snapshot(uint8_t* out, size_t capacity) { return refsave::snapshot(out, capacity); }
    int ctest_ref_reported() { return refsave::reported; }
    void ctest_ref_exit() {
        refsave::exited = true;
        refsave::ultramodern::join_saving_thread();
    }
}
