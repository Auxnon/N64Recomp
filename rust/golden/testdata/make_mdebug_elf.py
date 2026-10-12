"""Writes a small big-endian MIPS ELF with an .mdebug section, for exercising N64Recomp's
mdebug parsing (rust/golden/n64recomp-diff.sh). IDO-built ELFs carry mdebug; none are
available to test with, so this builds the structures by hand:

  a.c: global func_a (calls static helper), static helper, static data "shared"
  b.c: global func_b (calls its own static helper), static helper with a local symbol
       before its END, static data "shared"

The duplicate static names exercise renaming, and the local symbol exercises the C++'s
reuse of its pending symbol (the procedure is flushed early, with size 0).
"""
import struct
import sys

TEXT_VRAM = 0x80000400
TEXT_OFFSET = 0x100


def jal(target):
    return 0x0C000000 | ((target >> 2) & 0x03FFFFFF)


JR_RA, NOP = 0x03E00008, 0
ADDIU_SP = lambda imm: 0x27BD0000 | (imm & 0xFFFF)

# Code layout (vram):
#   0x400 func_a: addiu sp,-8; jal helper_a; nop; addiu sp,8; jr ra; nop
#   0x418 helper_a (static): jr ra; nop
#   0x420 func_b: jal helper_b; nop; jr ra; nop
#   0x430 helper_b (static): jr ra; nop
#   0x438 data: shared (a.c), shared (b.c)
code = [
    ADDIU_SP(-8), jal(0x80000418), NOP, ADDIU_SP(8), JR_RA, NOP,
    JR_RA, NOP,
    jal(0x80000430), NOP, JR_RA, NOP,
    JR_RA, NOP,
    0x11111111, 0x22222222,
]
text = b"".join(struct.pack(">I", w) for w in code)

# --- mdebug ---
ST_GLOBAL, ST_STATIC, ST_LOCAL, ST_PROC, ST_END, ST_STATICPROC = 1, 2, 4, 6, 8, 14
SC_TEXT, SC_DATA = 1, 2


def symr(iss, value, st, sc):
    return struct.pack(">iII", iss, value, (st << 26) | (sc << 21))


files = []  # (name, [(sym name, value, st, sc)])
files.append(("a.c", [
    ("func_a", 0x80000400, ST_PROC, SC_TEXT),
    ("", 0x18, ST_END, SC_TEXT),
    ("helper", 0x80000418, ST_STATICPROC, SC_TEXT),
    ("", 0x8, ST_END, SC_TEXT),
    ("shared", 0x80000438, ST_STATIC, SC_DATA),
]))
files.append(("b.c", [
    ("func_b", 0x80000420, ST_PROC, SC_TEXT),
    ("", 0x10, ST_END, SC_TEXT),
    ("helper", 0x80000430, ST_STATICPROC, SC_TEXT),
    ("local_var", 0x4, ST_LOCAL, SC_TEXT),
    ("", 0x8, ST_END, SC_TEXT),
    ("shared", 0x8000043C, ST_STATIC, SC_DATA),
]))

strings = b""
symrs = b""
fdrs = []
isym = 0
for fname, syms in files:
    iss_base = len(strings)
    local = fname.encode() + b"\0"
    rss = 0
    entries = []
    for name, value, st, sc in syms:
        iss = len(local)
        local += name.encode() + b"\0"
        entries.append(symr(iss, value, st, sc))
    strings += local
    symrs += b"".join(entries)
    fdrs.append((rss, iss_base, len(local), isym, len(entries)))
    isym += len(entries)

HDRR_SIZE, FDR_SIZE = 96, 72
mdebug_offset = TEXT_OFFSET + len(text)
mdebug_offset += -mdebug_offset % 4
fd_off = mdebug_offset + HDRR_SIZE
sym_off = fd_off + FDR_SIZE * len(fdrs)
ss_off = sym_off + len(symrs)

hdrr = struct.pack(">HH", 0x7009, 0) + struct.pack(
    ">23i",
    0, 0, 0,  # ilineMax, cbLine, cbLineOffset
    0, 0,  # idnMax, cbDnOffset
    0, 0,  # ipdMax, cbPdOffset
    isym, sym_off,  # isymMax, cbSymOffset
    0, 0,  # ioptMax, cbOptOffset
    0, 0,  # iauxMax, cbAuxOffset
    len(strings), ss_off,  # issMax, cbSsOffset
    0, 0,  # issExtMax, cbSsExtOffset
    len(fdrs), fd_off,  # ifdMax, cbFdOffset
    0, 0,  # crfd, cbRfdOffset
    0, 0,  # iextMax, cbExtOffset
)
assert len(hdrr) == HDRR_SIZE
fdr_bytes = b""
for rss, iss_base, cb_ss, isym_base, csym in fdrs:
    fdr_bytes += struct.pack(">I9i2H7i", TEXT_VRAM, rss, iss_base, cb_ss, isym_base, csym, 0, 0, 0, 0, 0, 0,
                             0, 0, 0, 0, 0, 0, 0)
    assert len(fdr_bytes) % FDR_SIZE == 0
mdebug = hdrr + fdr_bytes + symrs + strings

# --- symtab / strtab / shstrtab ---
strtab = b"\0func_a\0func_b\0"


def sym(name_off, value, size, info, shndx):
    return struct.pack(">IIIBBH", name_off, value, size, info, 0, shndx)


STB_GLOBAL, STT_FUNC = 1, 2
symtab = sym(0, 0, 0, 0, 0)
symtab += sym(1, 0x80000400, 0x18, (STB_GLOBAL << 4) | STT_FUNC, 1)
symtab += sym(8, 0x80000420, 0x10, (STB_GLOBAL << 4) | STT_FUNC, 1)

shstrtab = b"\0.text\0.mdebug\0.symtab\0.strtab\0.shstrtab\0"
names = {n: shstrtab.index(n.encode() + b"\0") for n in (".text", ".mdebug", ".symtab", ".strtab", ".shstrtab")}

blob = bytearray(TEXT_OFFSET)
blob += text
blob += b"\0" * (mdebug_offset - len(blob))
blob += mdebug
offsets = {}
for name, data in ((".symtab", symtab), (".strtab", strtab), (".shstrtab", shstrtab)):
    blob += b"\0" * (-len(blob) % 4)
    offsets[name] = len(blob)
    blob += data
blob += b"\0" * (-len(blob) % 4)
shoff = len(blob)


def shdr(name, kind, flags, addr, offset, size, link=0, info=0, align=4, entsize=0):
    return struct.pack(">10I", name, kind, flags, addr, offset, size, link, info, align, entsize)


SHT_PROGBITS, SHT_SYMTAB, SHT_STRTAB, SHT_MIPS_DEBUG = 1, 2, 3, 0x70000005
SHF_ALLOC, SHF_EXECINSTR = 2, 4
blob += shdr(0, 0, 0, 0, 0, 0, align=0)
blob += shdr(names[".text"], SHT_PROGBITS, SHF_ALLOC | SHF_EXECINSTR, TEXT_VRAM, TEXT_OFFSET, len(text))
blob += shdr(names[".mdebug"], SHT_MIPS_DEBUG, 0, 0, mdebug_offset, len(mdebug))
blob += shdr(names[".symtab"], SHT_SYMTAB, 0, 0, offsets[".symtab"], len(symtab), link=4, info=1, entsize=16)
blob += shdr(names[".strtab"], SHT_STRTAB, 0, 0, offsets[".strtab"], len(strtab), align=1)
blob += shdr(names[".shstrtab"], SHT_STRTAB, 0, 0, offsets[".shstrtab"], len(shstrtab), align=1)
phoff = len(blob)
# One PT_LOAD covering .text, loaded at ROM 0x1000.
blob += struct.pack(">8I", 1, TEXT_OFFSET, TEXT_VRAM, 0x1000, len(text), len(text), 5, 0x10)

ehdr = b"\x7fELF" + bytes([1, 2, 1, 0]) + b"\0" * 8
ehdr += struct.pack(">HHIIIIIHHHHHH", 2, 8, 1, TEXT_VRAM, phoff, shoff, 0x1000, 52, 32, 1, 40, 6, 5)
blob[:52] = ehdr

open(sys.argv[1], "wb").write(blob)
