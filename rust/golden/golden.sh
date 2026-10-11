#!/usr/bin/env bash
# Golden-output check for the recompiler port (docs/rust-port-plan.md, phases 0 and 4).
#
#   golden.sh record <zelda64recomp-dir> <N64Recomp> <RSPRecomp>
#   golden.sh check  <zelda64recomp-dir> <N64Recomp> <RSPRecomp>
#
# Runs the given tools on Zelda64Recomp's configs and records or checks SHA-256 hashes of
# everything they write. The tools run in a scratch directory that links to Zelda64Recomp's
# inputs, so the checkout's own outputs (and its build) are left alone. Only hashes are
# stored here: the output is derived from the ROM and must not be committed.
# patches.toml is included once Zelda64Recomp's build has produced patches/patches.elf.
set -euo pipefail

mode=${1:?record|check}
zelda=$(realpath "${2:?Zelda64Recomp dir}")
n64recomp=$(realpath "${3:?N64Recomp binary}")
rsprecomp=$(realpath "${4:?RSPRecomp binary}")
here=$(dirname "$(realpath "$0")")
manifest="$here/zelda64recomp-mm-us.sha256"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work"/{patches,rsp,RecompiledFuncs,RecompiledPatches}
cp "$zelda"/{us.rev1.toml,aspMain.us.rev1.toml,njpgdspMain.us.rev1.toml,patches.toml} "$work"/
ln -s "$zelda"/{Zelda64RecompSyms,overlays.us.rev1.txt,mm.us.rev1.rom_uncompressed.z64} "$work"/
ln -s "$zelda"/patches/custom_syms.toml "$work"/patches/

cd "$work"
"$n64recomp" us.rev1.toml >/dev/null
"$rsprecomp" aspMain.us.rev1.toml >/dev/null
"$rsprecomp" njpgdspMain.us.rev1.toml >/dev/null
outputs=(RecompiledFuncs rsp)
if [[ -f "$zelda"/patches/patches.elf ]]; then
    ln -s "$zelda"/patches/patches.elf patches/
    "$n64recomp" patches.toml >/dev/null
    outputs+=(RecompiledPatches patches/patches.bin)
fi

hashes=$(find "${outputs[@]}" -type f -print0 | LC_ALL=C sort -z | xargs -0 sha256sum)

case "$mode" in
record)
    {
        echo "# Zelda64Recomp $(git -C "$zelda" rev-parse --short HEAD), N64Recomp $(git -C "$here" rev-parse --short HEAD)"
        echo "$hashes"
    } >"$manifest"
    echo "recorded $(wc -l <<<"$hashes") files to $manifest"
    ;;
check)
    if diff <(grep -v '^#' "$manifest") <(echo "$hashes"); then
        echo "golden output matches ($(wc -l <<<"$hashes") files)"
    else
        echo "golden output differs (lines above: < expected, > actual)" >&2
        exit 1
    fi
    ;;
*)
    echo "unknown mode: $mode" >&2
    exit 2
    ;;
esac
