#!/usr/bin/env bash
# Compares two N64Recomp builds on config variants the golden corpus doesn't cover
# (docs/rust-port-plan.md, phase 4).
#
#   n64recomp-diff.sh <N64Recomp-a> <N64Recomp-b> [zelda64recomp-dir] [gauntlet-recomp-dir] [gauntlet-decomp-dir]
#
# The directories default to siblings of this repo; cases whose inputs are missing are
# skipped. Each case runs both tools in their own scratch directory with the same layout
# and compares the exit status and every file written. Nothing derived from a ROM is
# written outside the scratch directory.
set -uo pipefail

tool_a=$(realpath "${1:?N64Recomp A}")
tool_b=$(realpath "${2:?N64Recomp B}")
here=$(dirname "$(realpath "$0")")
repo=$(dirname "$(dirname "$here")")
zelda=$(realpath -m "${3:-$repo/../Zelda64Recomp}")
gauntlet=$(realpath -m "${4:-$repo/../gauntlet-legends-recomp}")
gauntlet_decomp=$(realpath -m "${5:-$repo/../gauntlet-legends}")

work=$(mktemp -d)
if [[ -n "${KEEP_WORK:-}" ]]; then echo "keeping $work"; else trap 'rm -rf "$work"' EXIT; fi
failures=0
cases=0

# MM symbol-file variants: one file per function, trace mode, hooks, stubs, renames.
setup_mm_variants() {
    ln -s "$zelda"/{mm.us.rev1.rom_uncompressed.z64,overlays.us.rev1.txt} .
    mkdir Zelda64RecompSyms
    # Drop some symbols so calls to them create static functions, and duplicate the
    # non-relocatable boot section so calls into it from elsewhere become ambiguous lookups.
    python3 -I - "$zelda"/Zelda64RecompSyms/mm.us.rev1.syms.toml Zelda64RecompSyms/mm.us.rev1.syms.toml <<'PY'
import re, sys
text = open(sys.argv[1]).read()
for name in ("Graph_Update", "Graph_InitTHGA", "Graph_UpdateGame"):
    text, n = re.subn(r'\n    \{ name = "%s", [^\n]*' % name, "", text)
    assert n == 1, name
boot = re.search(r'\[\[section\]\]\nname = "\.\.boot".*?(?=\[\[section\]\])', text, re.S).group(0)
copy = boot.replace('name = "..boot"', 'name = "..boot_copy"')
# The copies keep their names, so the built-in reimplemented/ignored lists apply to them.
open(sys.argv[2], "w").write(text + "\n" + copy)
PY
    sed -e 's|^\[patches\]|[patches]\nrenamed = ["Graph_FaultClient"]|' \
        -e 's|^output_func_path = .*|output_func_path = "out"\nfunctions_per_output_file = 1\ntrace_mode = true|' \
        "$zelda"/us.rev1.toml >variants.toml
    cat >>variants.toml <<'TOML'

[[patches.hook]]
func = "Graph_TaskSet00"
text = "    hook_at_start();"

[[patches.hook]]
func = "Graph_TaskSet00"
before_vram = 0x801741C0
text = "    hook_in_middle();"

[[patches.hook]]
func = "AudioHeap_WritebackDCache"
before_vram = 0x8018B4F8
text = "    hook_at_first_instruction();"
TOML
}

# MM patches ELF: relocations, reference symbols, exports and events, batched output and
# size overrides. Needs Zelda64Recomp's built patches/patches.elf.
setup_mm_patches() {
    mkdir patches
    ln -s "$zelda"/Zelda64RecompSyms .
    ln -s "$zelda"/patches/{patches.elf,custom_syms.toml} patches/
    sed -e 's|^output_func_path = .*|output_func_path = "out"|' \
        -e 's|^single_file_output = .*|functions_per_output_file = 7|' \
        -e 's|^output_binary_path = .*|output_binary_path = "out/patches.bin"|' \
        "$zelda"/patches.toml >patches_batched.toml
    cat >>patches_batched.toml <<'TOML'
function_sizes = [ { name = "memcpy", size = 0x20 } ]
TOML
    # A manual function covering the start of .text.
    local text_vram
    text_vram=$(readelf -SW patches/patches.elf | sed -E 's/^ *\[ *[0-9]+\] //' | awk '$1 == ".text" { print $3 }')
    printf 'manual_funcs = [ { name = "manual_test_func", section = ".text", vram = 0x%s, size = 0x10 } ]\n' \
        "$text_vram" >>patches_batched.toml
}

setup_mm_dump() {
    mkdir patches
    ln -s "$zelda"/Zelda64RecompSyms .
    ln -s "$zelda"/patches/{patches.elf,custom_syms.toml} patches/
    sed -e 's|^output_func_path = .*|output_func_path = "out"|' "$zelda"/patches.toml >dump.toml.in
}

# Gauntlet's symbol-file config, as the spike uses it (TLB-mapped code, ambiguous calls).
setup_gauntlet() {
    mkdir generated
    ln -s "$gauntlet"/generated/{gauntlet.syms.toml,gauntlet.recomp.z64} generated/
    sed -e 's|^output_func_path = .*|output_func_path = "out"|' "$gauntlet"/gauntlet.us.toml >gauntlet.toml
}

# Gauntlet decomp ELFs in ELF mode: symbol reading and context dumps without relocations.
setup_gauntlet_elf() {
    ln -s "$gauntlet_decomp"/build/{boot.elf,game.elf} .
    for elf in boot game; do
        cat >$elf.toml <<TOML
[input]
elf_path = "$elf.elf"
output_func_path = "out_$elf"
use_absolute_symbols = true
TOML
    done
    # The spike's ignore list covers functions the recompiler can't handle; keep the ones
    # each ELF defines (unknown names are errors).
    for elf in boot game; do
        local names
        readelf -sW $elf.elf >symbols.txt
        names=$(sed -n '/^ignored = \[/,/^\]/p' "$gauntlet"/gauntlet.us.toml | grep -o '"[A-Za-z0-9_]*"' |
            while read -r name; do
                grep -qw "${name//\"/}" symbols.txt && printf '%s, ' "$name"
            done)
        rm symbols.txt
        printf '\n[patches]\nignored = [%s]\n' "$names" >>$elf.toml
    done
}

# A generated ELF with an .mdebug section (see testdata/make_mdebug_elf.py).
setup_mdebug() {
    python3 -I "$here"/testdata/make_mdebug_elf.py md.elf
    printf '[input]\nelf_path = "md.elf"\noutput_func_path = "out"\nuse_mdebug = true\n' >md.toml
}

# run_case <name> <setup> <input that must exist> <command>...
# The command runs in the case's scratch directory with $tool set to the tool under test.
run_case() {
    local name=$1 setup=$2 input=$3
    shift 3
    if [[ ! -e "$input" ]]; then
        echo "$name: skipped ($input not found)"
        return
    fi
    cases=$((cases + 1))
    local status=()
    for side in a b; do
        local dir="$work/$name.$side"
        mkdir -p "$dir/run"
        local tool_var=tool_$side
        (cd "$dir/run" && "$setup" && tool=${!tool_var} && eval "$*" >../stdout.txt 2>../stderr.txt)
        status+=($?)
    done
    local a="$work/$name.a" b="$work/$name.b"
    if [[ ${status[0]} != "${status[1]}" ]]; then
        echo "$name: DIFFERENT exit status (${status[0]} vs ${status[1]})" >&2
        failures=$((failures + 1))
    elif ! diff -q "$a/stderr.txt" "$b/stderr.txt" >/dev/null; then
        echo "$name: DIFFERENT stderr" >&2
        diff "$a/stderr.txt" "$b/stderr.txt" | head -20 >&2
        failures=$((failures + 1))
    elif [[ ${status[0]} != 0 ]]; then
        # Both failed the same way. Their partial output isn't comparable: the C++ exits
        # without flushing its output streams.
        echo "$name: identical failure (exit ${status[0]}: $(tail -n 1 "$a/stderr.txt"))"
    elif ! diff -r --no-dereference -q "$a/run" "$b/run" >/dev/null; then
        echo "$name: DIFFERENT output" >&2
        diff -r --no-dereference "$a/run" "$b/run" | head -20 >&2
        failures=$((failures + 1))
    else
        echo "$name: identical (exit 0, $(find "$a/run" -type f | wc -l) files)"
    fi
}

run_case mm-variants setup_mm_variants "$zelda/us.rev1.toml" '"$tool" variants.toml'
run_case mm-patches-batched setup_mm_patches "$zelda/patches/patches.elf" '"$tool" patches_batched.toml'
run_case mm-patches-dump setup_mm_dump "$zelda/patches/patches.elf" '"$tool" dump.toml.in --dump-context'
run_case mdebug setup_mdebug "$here/testdata/make_mdebug_elf.py" '"$tool" md.toml'
run_case mdebug-dump setup_mdebug "$here/testdata/make_mdebug_elf.py" '"$tool" md.toml --dump-context'
run_case gauntlet setup_gauntlet "$gauntlet/gauntlet.us.toml" '"$tool" gauntlet.toml'
run_case gauntlet-boot-elf setup_gauntlet_elf "$gauntlet_decomp/build/boot.elf" '"$tool" boot.toml'
run_case gauntlet-boot-dump setup_gauntlet_elf "$gauntlet_decomp/build/boot.elf" '"$tool" boot.toml --dump-context'
run_case gauntlet-game-elf setup_gauntlet_elf "$gauntlet_decomp/build/game.elf" '"$tool" game.toml'
run_case gauntlet-game-dump setup_gauntlet_elf "$gauntlet_decomp/build/game.elf" '"$tool" game.toml --dump-context'

echo "$cases cases, $failures different"
[[ $failures == 0 ]]
