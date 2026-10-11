#!/usr/bin/env bash
# Compares two RSPRecomp builds on synthetic configs (docs/rust-port-plan.md, phase 4).
#
#   rsp-diff.sh <zelda64recomp-dir> <RSPRecomp-a> <RSPRecomp-b>
#
# MM's own RSP configs don't use overlay slots or unsupported_instructions, so the golden
# corpus doesn't cover those paths. These configs carve made-up overlays out of MM's
# aspMain and njpgdspMain text and mark some instructions unsupported. The generated code
# isn't meant to run; it only has to come out identical from both tools. Nothing derived
# from the ROM is written outside a temporary directory.
set -euo pipefail

zelda=$(realpath "${1:?Zelda64Recomp dir}")
tool_a=$(realpath "${2:?RSPRecomp A}")
tool_b=$(realpath "${3:?RSPRecomp B}")

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
ln -s "$zelda"/mm.us.rev1.rom_uncompressed.z64 "$work"/rom.z64

cat >"$work"/two_slots.toml <<'TOML'
text_offset = 0xC40FF0
text_size = 0x1000
text_address = 0x04001000
rom_file_path = "rom.z64"
output_file_path = "out/two_slots.cpp"
output_function_name = "twoSlots"
extra_indirect_branch_targets = [0x1F80, 0x1250, 0x1154, 0x1094]
unsupported_instructions = [0x1004, 0x1250, 0x1808, 0x1F80, 0x1000]
[[overlay_slots]]
text_address = 0x04001800
overlays = [
    { offset = 0x800, size = 0x100 },
    { offset = 0x400, size = 0x100 },
]
[[overlay_slots]]
text_address = 0x04001C00
overlays = [
    { offset = 0xC00, size = 0x80 },
    { offset = 0x000, size = 0x80 },
    { offset = 0x600, size = 0x80 },
]
TOML

cat >"$work"/one_slot.toml <<'TOML'
text_offset = 0xC438B0
text_size = 0xAF0
text_address = 0x04001080
rom_file_path = "rom.z64"
output_file_path = "out/one_slot.cpp"
output_function_name = "oneSlot"
[[overlay_slots]]
text_address = 0x04001400
overlays = [
    { offset = 0x380, size = 0x200 },
    { offset = 0x000, size = 0x200 },
    { offset = 0x780, size = 0x300 },
]
TOML

status=0
for config in two_slots one_slot; do
    for tool in a b; do
        bin=tool_$tool
        (cd "$work" && "${!bin}" $config.toml >/dev/null && mv out/$config.cpp $config.$tool.cpp)
    done
    if cmp -s "$work"/$config.a.cpp "$work"/$config.b.cpp; then
        echo "$config: identical ($(grep -c '^R_\|^L_' "$work"/$config.a.cpp) labels, $(wc -l <"$work"/$config.a.cpp) lines)"
    else
        echo "$config: DIFFERENT" >&2
        diff "$work"/$config.a.cpp "$work"/$config.b.cpp | head -20 >&2
        status=1
    fi
done
exit $status
