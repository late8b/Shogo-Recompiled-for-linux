#!/usr/bin/env python3
"""Disassemble a VA range of Client.exe (PE32) with capstone.

Used to recover the exact argument sequences of the LithTech stream vtable
calls in FUN_00418d80, which Ghidra's C output mangles.

Usage: dis_pe.py <pe> <hexVA> [nbytes]
"""
import struct
import sys

import capstone


def load_sections(path):
    d = open(path, 'rb').read()
    pe = struct.unpack_from('<I', d, 0x3c)[0]
    assert d[pe:pe + 4] == b'PE\0\0', 'not a PE'
    nsec = struct.unpack_from('<H', d, pe + 6)[0]
    optsz = struct.unpack_from('<H', d, pe + 20)[0]
    opt = pe + 24
    magic = struct.unpack_from('<H', d, opt)[0]
    imagebase = struct.unpack_from('<I', d, opt + 28)[0]
    secs = []
    sh = opt + optsz
    for i in range(nsec):
        b = sh + i * 40
        name = d[b:b + 8].rstrip(b'\0').decode('latin-1')
        vsz, va, rsz, praw = struct.unpack_from('<IIII', d, b + 8)
        secs.append((name, va, vsz, praw, rsz))
    return d, magic, imagebase, secs


def va_to_off(va, imagebase, secs):
    rva = va - imagebase
    for name, sva, vsz, praw, rsz in secs:
        if sva <= rva < sva + max(vsz, rsz):
            return praw + (rva - sva), name
    return None, None


def disasm(d, imagebase, secs, va, nbytes):
    off, sec = va_to_off(va, imagebase, secs)
    if off is None:
        return None, None
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    md.detail = True
    return list(md.disasm(d[off:off + nbytes], va)), sec


def main(argv):
    if argv and argv[0] == '--reads':
        pe = argv[1]
        va = int(argv[2], 16)
        nbytes = int(argv[3], 0) if len(argv) > 3 else 0x1800
        d, magic, imagebase, secs = load_sections(pe)
        insns, sec = disasm(d, imagebase, secs, va, nbytes)
        pending = []
        for ins in insns:
            if ins.mnemonic == 'push':
                pending.append((ins.address, ins.op_str))
                continue
            if ins.mnemonic == 'call' and ('+ 8]' in ins.op_str or '+ 0xc]' in ins.op_str):
                dst = pending[-1][1] if pending else '?'
                size = pending[-2][1] if len(pending) >= 2 else '?'
                kind = 'STR ' if '+ 0xc]' in ins.op_str else 'READ'
                print('  %08x  %s  dst=%-16s size=%-8s' % (ins.address, kind, dst, size))
                pending = []
                continue
            if ins.mnemonic in ('mov', 'lea', 'add', 'cmp', 'test', 'jmp', 'je', 'jne',
                                'jz', 'jnz', 'sub', 'xor'):
                pending = pending[-4:]
                continue
            pending = []
        return
    pe = argv[0]
    va = int(argv[1], 16)
    nbytes = int(argv[2], 0) if len(argv) > 2 else 0x1800
    d, magic, imagebase, secs = load_sections(pe)
    print('# magic=%#x imagebase=%#x nsec=%d' % (magic, imagebase, len(secs)))
    for s in secs:
        print('#  %-8s va=%#010x vsz=%#08x raw=%#08x rsz=%#08x' % s)
    insns, sec = disasm(d, imagebase, secs, va, nbytes)
    if insns is None:
        print('VA not mapped'); return
    off, _ = va_to_off(va, imagebase, secs)
    print('# VA %#x -> file %#x (section %s)' % (va, off, sec))
    for ins in insns:
        print('  %08x  %-8s %s' % (ins.address, ins.mnemonic, ins.op_str))
        if ins.mnemonic == 'ret':
            print('  ; ---- first ret at %#x ----' % ins.address)
            break
    print('# magic=%#x imagebase=%#x nsec=%d' % (magic, imagebase, len(secs)))
    for s in secs:
        print('#  %-8s va=%#010x vsz=%#08x raw=%#08x rsz=%#08x' % s)
    insns, sec = disasm(d, imagebase, secs, va, nbytes)
    if insns is None:
        print('VA not mapped'); return
    off, _ = va_to_off(va, imagebase, secs)
    print('# VA %#x -> file %#x (section %s)' % (va, off, sec))
    for ins in insns:
        print('  %08x  %-8s %s' % (ins.address, ins.mnemonic, ins.op_str))
        if ins.mnemonic == 'ret':
            print('  ; ---- first ret at %#x ----' % ins.address)
            break


if __name__ == '__main__':
    main(sys.argv[1:])
