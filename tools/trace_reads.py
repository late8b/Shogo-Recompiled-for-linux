#!/usr/bin/env python3
"""Resolve the stream vtable reads in FUN_00418d80 of Client.exe by
register tracking (linear pass, LEA-aware). Prints the executed read
sequence with resolved destinations (esp+..., ebx+...) and sizes.

Usage: trace_reads.py <pe> <va> <nbytes>
"""
import sys
import struct

import capstone


def load_sections(path):
    d = open(path, 'rb').read()
    pe = struct.unpack_from('<I', d, 0x3c)[0]
    nsec = struct.unpack_from('<H', d, pe + 6)[0]
    optsz = struct.unpack_from('<H', d, pe + 20)[0]
    opt = pe + 24
    imagebase = struct.unpack_from('<I', d, opt + 28)[0]
    secs = []
    sh = opt + optsz
    for i in range(nsec):
        b = sh + i * 40
        name = d[b:b + 8].rstrip(b'\0').decode('latin-1')
        vsz, va, rsz, praw = struct.unpack_from('<IIII', d, b + 8)
        secs.append((name, va, vsz, praw, rsz))
    return d, imagebase, secs


def va_to_off(va, imagebase, secs):
    rva = va - imagebase
    for name, sva, vsz, praw, rsz in secs:
        if sva <= rva < sva + max(vsz, rsz):
            return praw + (rva - sva)
    return None


REGS = ('eax', 'ecx', 'edx', 'ebx', 'esp', 'ebp', 'esi', 'edi')


def main(argv):
    pe, va_s, nb_s = argv[0], argv[1], argv[2]
    va = int(va_s, 16)
    nbytes = int(nb_s, 0)
    d, imagebase, secs = load_sections(pe)
    off = va_to_off(va, imagebase, secs)
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    md.detail = True
    insns = list(md.disasm(d[off:off + nbytes], va))
    reg = {r: None for r in REGS}      # resolved frame reference
    regv = {r: None for r in REGS}     # known constant value
    pending = []                        # (text, resolved-or-None)
    seen_branches = set()

    def resolve(tok):
        tok = tok.strip()
        if len(tok) == 3 and tok in REGS:
            if reg[tok]:
                return reg[tok]
            if regv[tok] is not None:
                return str(regv[tok])
            return tok + '?'
        return tok

    for ins in insns:
        m, o = ins.mnemonic, ins.op_str
        if m == 'lea':
            dst, src = o.split(',', 1)
            dst = dst.strip()
            src = src.strip()
            # [esp + 0xNN] style
            if src.startswith('[') and src.endswith(']'):
                inner = src[1:-1]
                parts = inner.replace(' ', '').split('+')
                if len(parts) == 2 and parts[0] in REGS:
                    reg[dst] = '%s+%s' % (parts[0], parts[1])
                elif len(parts) == 1 and parts[0] in REGS:
                    reg[dst] = parts[0]
                elif len(parts) == 2 and parts[0] in REGS and '-' in inner:
                    reg[dst] = None
            continue
        if m == 'mov':
            dst, src = o.split(',', 1)
            dst = dst.strip()
            if dst in REGS:
                if src.strip().startswith('0x') or src.strip().isdigit():
                    regv[dst] = src.strip()
                    reg[dst] = None
                elif src.strip() in REGS:
                    regv[dst] = regv[src.strip()]
                    reg[dst] = reg[src.strip()]
                else:
                    regv[dst] = None
            continue
        if m == 'push':
            tok = o
            pending.append((tok, resolve(tok), ins.address))
            continue
        if m == 'call':
            if '+ 8]' in o or '+ 0xc]' in o:
                # dst = last push, size = prev push
                last = pending[-1] if pending else ('?', '?', 0)
                prev = pending[-2] if len(pending) >= 2 else ('?', '?', 0)
                kind = 'STR' if '+ 0xc]' in o else 'READ'
                print('%08x %s  dst=%-16s size=%-10s  (raw: %s, %s)' %
                      (ins.address, kind, last[1], prev[1], last[0], prev[0]))
                pending = []
                # conservatively clear operand regs (callee may clobber)
                for r in ('eax', 'ecx', 'edx', 'esi', 'edi'):
                    reg[r] = None
                    regv[r] = None
            elif o.startswith('0x') or o.startswith('-'):
                # direct call: may clobber regs
                for r in ('eax', 'ecx', 'edx', 'esi', 'edi'):
                    reg[r] = None
                    regv[r] = None
                pending = pending[:0]
            else:
                # indirect non-vtable call: clear everything
                reg = {r: None for r in REGS}
                regv = {r: None for r in REGS}
                pending = []
            continue
        if m.startswith('j') or m == 'loop':
            continue
        # data ops do not disturb our tracking of frame refs except they may be
        # interleaved; keep pending bounded
        if m in ('add', 'sub', 'inc', 'dec', 'and', 'or', 'xor', 'shl', 'shr',
                 'test', 'cmp', 'imul', 'mul', 'not', 'neg', 'sar', 'sbb', 'adc'):
            if o:
                dst = o.split(',')[0].strip()
                if dst in REGS:
                    reg[dst] = None
                    regv[dst] = None
            continue
        # anything else clears nothing but may consume pushes
        pending = []
    # next-function boundary check
    print('; end of linear scan')


if __name__ == '__main__':
    main(sys.argv[1:])