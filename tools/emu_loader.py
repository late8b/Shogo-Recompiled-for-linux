#!/usr/bin/env python3
"""Emulate Client.exe!FUN_00418d80 (the .DAT world-geometry loader) under Unicorn
with a synthetic stream object whose vtable we intercept.  This yields the exact
ordered sequence of stream reads (dst,size) the real loader performs, which is
the ground truth for the on-disk .DAT layout.

Usage: emu_loader.py <Client.exe> <world.dat>
"""
import struct
import sys

from unicorn import *
from unicorn.x86_const import *
import capstone

# ---- PE -------------------------------------------------------------------
class PE:
    def __init__(self, path):
        self.d = open(path, 'rb').read()
        pe = struct.unpack_from('<I', self.d, 0x3c)[0]
        self.imagebase = struct.unpack_from('<I', self.d, pe + 0x34)[0]
        nsec = struct.unpack_from('<H', self.d, pe + 0x06)[0]
        opt = struct.unpack_from('<H', self.d, pe + 0x14)[0]
        self.sections = []
        off = pe + 0x18 + opt
        for i in range(nsec):
            name = self.d[off:off + 8].rstrip(b'\0').decode('latin1')
            vsize, va, rsize, raw = struct.unpack_from('<IIII', self.d, off + 8)
            self.sections.append((name, va, vsize, raw, rsize))
            off += 0x28
        self.sizeofimage = max(va + vsize for _, va, vsize, _, _ in self.sections)


# ---- memory helpers -------------------------------------------------------
BASE = 0x400000
STACK_LO, STACK_TOP = 0x01000000, 0x01400000
SCRATCH = 0x30000000
HEAP = 0x40000000
STUB = 0x60000000

STUB_READ = STUB + 0x00
STUB_READSTR = STUB + 0x10
STUB_TELL = STUB + 0x20
STUB_SEEK = STUB + 0x30
STUB_EOF = STUB + 0x40
STUB_OTHER = STUB + 0x50

STREAM_OBJ = SCRATCH + 0x1000
VTABLE = SCRATCH + 0x2000
STREAMPTR = SCRATCH + 0x0000   # word holding STREAM_OBJ
ARG_OBJ = SCRATCH + 0x4000
WRAPOBJ = SCRATCH + 0x6000     # arg1: wrapper with vtable at +8
VTABLE2 = SCRATCH + 0x7000
STUB_WREAD = STUB + 0x60


def main():
    if len(sys.argv) < 3:
        print(__doc__)
        return 1
    pe = PE(sys.argv[1])
    filedata = open(sys.argv[2], 'rb').read()

    uc = Uc(UC_ARCH_X86, UC_MODE_32)

    def map_(addr, size, perms=UC_PROT_ALL):
        uc.mem_map(addr & ~0xfff, ((size + 0xfff) & ~0xfff) or 0x1000, perms)

    # map image
    imgsize = (pe.sizeofimage + 0xfff) & ~0xfff
    map_(BASE, imgsize)
    uc.mem_write(BASE, pe.d[:min(len(pe.d), imgsize)])
    for name, va, vsize, raw, rsize in pe.sections:
        if rsize:
            uc.mem_write(BASE + va, pe.d[raw:raw + rsize])
    # stacks / scratch / heap / stubs
    map_(STACK_LO, STACK_TOP - STACK_LO)
    map_(SCRATCH, 0x10000)
    map_(HEAP, 0x1000000)
    map_(STUB, 0x10000)
    uc.mem_write(STUB, b'\xcc' * 0x1000)

    # stream vtable
    uc.mem_write(STREAMPTR, struct.pack('<I', STREAM_OBJ))
    vt = [0, 0, STUB_READ, STUB_READSTR, STUB_EOF, STUB_SEEK, STUB_TELL, STUB_OTHER]
    uc.mem_write(VTABLE, b''.join(struct.pack('<I', x) for x in vt))
    uc.mem_write(STREAM_OBJ, struct.pack('<I', VTABLE))
    # wrapper object (arg1): [0]=stream, [8]=method ptr, [0xc]=4
    uc.mem_write(WRAPOBJ, struct.pack('<III', STREAM_OBJ, 0, STUB_WREAD))
    uc.mem_write(WRAPOBJ + 0xc, struct.pack('<I', 4))

    state = {'pos': 0, 'log': [], 'heap': HEAP, 'trace': []}
    state['pos'] = struct.unpack_from('<I', filedata, 8)[0]
    state['fsize'] = len(filedata)

    def rd32(a):
        return struct.unpack('<I', uc.mem_read(a, 4))[0]

    def wr32(a, v):
        uc.mem_write(a, struct.pack('<I', v))

    # ---- stub hook (vtable methods) ----
    STUBBED = {0x4171a0: 0, 0x4171b0: 0}

    DBG = {0x419089, 0x419270, 0x419319, 0x419226}

    def hook_code(uc, addr, size, user):
        if addr in DBG:
            esp = uc.reg_read(UC_X86_REG_ESP)
            vals = {o: rd32(esp + o) for o in range(0x10, 0xa0, 4)}
            keys = {o: v for o, v in vals.items() if v in (202605, 682, 148, 898, 279, 2689, 5101, 123)}
            print('DBG 0x%x esp=0x%x slots=%s' % (addr, esp, keys))
        if addr in STUBBED:
            esp = uc.reg_read(UC_X86_REG_ESP)
            uc.reg_write(UC_X86_REG_EAX, STUBBED[addr])
            uc.reg_write(UC_X86_REG_ESP, esp + 4)
            uc.reg_write(UC_X86_REG_EIP, rd32(esp))
            return
        if not (STUB <= addr < STUB + 0x10000):
            return
        esp = uc.reg_read(UC_X86_REG_ESP)
        ret = rd32(esp)
        a1 = rd32(esp + 4)
        a2 = rd32(esp + 8)
        p = state['pos']
        if p > state['fsize'] + 0x200000:
            print('RUNAWAY pos=%d (fsize=%d)' % (p, state['fsize']))
            uc.emu_stop()
            return

        def eof_read(dst, size):
            """Copy `size` bytes from the stream, zero-filling past EOF.

            The real stream returns 0-filled/short data at EOF; the previous
            version left the destination untouched, so stale loop counters
            caused unbounded reads (the RUNAWAY).  Zero-filling lets the loader
            see 0 counts and terminate, and we record the first EOF crossing.
            """
            if size < 0:
                size = 0
            avail = max(0, min(size, len(filedata) - p))
            if avail:
                uc.mem_write(dst, filedata[p:p + avail])
            if avail < size:
                uc.mem_write(dst + avail, b'\0' * (size - avail))
                if not state.get('eof_first'):
                    state['eof_first'] = (p, size, ret)
                    print('EOF CROSSED first at pos=%d size=%d ret=0x%x' % (p, size, ret))

        if addr == STUB_READ:
            eof_read(a1, a2)
            state['pos'] = p + a2
            state['log'].append(('READ', p, a2, ret, a1, esp))
            uc.reg_write(UC_X86_REG_ESP, esp + 12)   # ret 8 (thiscall stdcall)
            uc.reg_write(UC_X86_REG_EIP, ret)
            return
        elif addr == STUB_READSTR:
            q = p
            out = bytearray()
            while len(out) < a2 and q < len(filedata):
                c = filedata[q]
                q += 1
                out.append(c)
                if c == 0:
                    break
            uc.mem_write(a1, bytes(out) + b'\0')
            state['pos'] = q
            state['log'].append(('STR', p, q - p, ret))
            uc.reg_write(UC_X86_REG_ESP, esp + 12)
            uc.reg_write(UC_X86_REG_EIP, ret)
            return
        elif addr == STUB_TELL:
            if a1:
                wr32(a1, p)
            uc.reg_write(UC_X86_REG_EAX, p)
            state['log'].append(('TELL', p, 0, ret))
            uc.reg_write(UC_X86_REG_ESP, esp + 8)     # ret 4
            uc.reg_write(UC_X86_REG_EIP, ret)
            return
        elif addr == STUB_SEEK:
            state['pos'] = a1
            state['log'].append(('SEEK', p, a1, ret))
            uc.reg_write(UC_X86_REG_ESP, esp + 8)    # ret 4
            uc.reg_write(UC_X86_REG_EIP, ret)
            return
        elif addr == STUB_EOF:
            uc.reg_write(UC_X86_REG_EAX, 1 if p >= len(filedata) else 0)
            state['log'].append(('EOF', p, 0, ret))
            uc.reg_write(UC_X86_REG_ESP, esp + 4)
            uc.reg_write(UC_X86_REG_EIP, ret)
            return
        elif addr == STUB_WREAD:
            # wrapper[8](wrapper[0xc]) callback; empirically consumes wrapper[0xc]=4
            eof_read(SCRATCH + 0x9000, a1)
            state['pos'] = p + a1
            state['log'].append(('WREAD', p, a1, ret))
            uc.reg_write(UC_X86_REG_ESP, esp + 4)
            uc.reg_write(UC_X86_REG_EIP, ret)
            return
        else:
            uc.reg_write(UC_X86_REG_EAX, 0)
        uc.reg_write(UC_X86_REG_ESP, esp + 4)
        uc.reg_write(UC_X86_REG_EIP, ret)

    uc.hook_add(UC_HOOK_CODE, hook_code)

    # ---- malloc hook ----
    def hook_malloc(uc, addr, size, user):
        esp = uc.reg_read(UC_X86_REG_ESP)
        n = rd32(esp + 4)
        state.setdefault('allocs', []).append((uc.reg_read(UC_X86_REG_EIP), n))
        # align, add guard
        p = (state['heap'] + 0xf) & ~0xf
        state['heap'] = p + max(n, 16) + 0x10
        if state['heap'] > HEAP + 0xF00000:
            print('HEAP EXHAUSTED; allocs: %r' % (state['allocs'][-6:],))
            print('READ LOG SO FAR:')
            for t in state['log']:
                print('   ', t)
            raise RuntimeError('heap exhausted')
        uc.mem_write(p, b'\0' * max(n, 1))
        uc.reg_write(UC_X86_REG_EAX, p)
        uc.reg_write(UC_X86_REG_ESP, esp + 4)
        uc.reg_write(UC_X86_REG_EIP, rd32(esp))

    for a in (0x41b880, 0x41b800):
        uc.hook_add(UC_HOOK_CODE, hook_malloc, begin=a, end=a)

    # ---- set up call frame ----
    # args as the real caller pushes: ebx, ebp, 1, ebp+4, local, local
    esp = STACK_TOP - 0x1000
    args = [WRAPOBJ, ARG_OBJ, 1, ARG_OBJ + 4, SCRATCH + 0x8000, SCRATCH + 0x8004]
    retaddr = 0x0BADF00D
    for a in reversed(args):
        esp -= 4
        wr32(esp, a)
    esp -= 4
    wr32(esp, retaddr)
    uc.reg_write(UC_X86_REG_ESP, esp)
    uc.reg_write(UC_X86_REG_EIP, 0x418d80)

    map_(retaddr & ~0xfff, 0x1000)
    uc.mem_write(retaddr, b'\xf4' * 16)  # hlt

    def hook_invalid(uc, access, address, size, value, user):
        print('MEM_INVALID access=%d addr=0x%x size=%d pc=0x%x' %
              (access, address, size, uc.reg_read(UC_X86_REG_EIP)))
        uc.emu_stop()

    uc.hook_add(UC_HOOK_MEM_INVALID, hook_invalid)

    def hook_fetch(uc, address, size, user):
        print('FETCH_INVALID addr=0x%x eip=0x%x esp=0x%x' %
              (address, uc.reg_read(UC_X86_REG_EIP), uc.reg_read(UC_X86_REG_ESP)))
        uc.emu_stop()

    uc.hook_add(UC_HOOK_MEM_FETCH_INVALID, hook_fetch)

    last = {'n': 0}

    def hook_log(uc, addr, size, user):
        if last['n'] < 60:
            print('  pc=0x%x esp=0x%x' % (addr, uc.reg_read(UC_X86_REG_ESP)))
        last['n'] += 1

    uc.hook_add(UC_HOOK_CODE, hook_log)

    steps = 0
    def hook_count(uc, addr, size, user):
        nonlocal steps
        steps += 1
        if steps > 20000000:
            uc.emu_stop()

    uc.hook_add(UC_HOOK_CODE, hook_count)

    try:
        uc.emu_start(0x418d80, retaddr, timeout=120 * 1000000, count=20000000)
    except UcError as e:
        print('UC ERROR:', e)
    print('final stream pos:', state['pos'], 'of', len(filedata))
    for t in state['log']:
        if t[0] == 'READ' and len(t) >= 6:
            print('%s pos=%d size=%d ret=0x%x dst=esp%+d' % (t[0], t[1], t[2], t[3], t[4] - t[5]))
        else:
            print(t)
    return 0


if __name__ == '__main__':
    sys.exit(main())
