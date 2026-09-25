#!/usr/bin/env python3
"""Compare the memory two ROMs leave behind after Kobo's emulator loads the same levels.

    ramdiff.py a.smc b.smc 105 106 ...        byte runs that differ, per level
    ramdiff.py --summary a.smc b.smc all      addresses that differ, counted over levels
    ramdiff.py --revert 05D8E2-05D8E5 vanilla.smc lm.smc 105
                                              ablation: b with those ranges put back to
                                              a's bytes, against b itself, to see what the
                                              reverted hook leaves behind
    options: --kobo PATH (default: kobo on PATH), --vram (also VRAM and CGRAM)

Clean room (docs/step-2.md): this reports memory effects only, addresses and the
values each ROM left there after `kobo level wram` / `kobo level dump` ran its load.
It never prints instructions, program counters, or ROM bytes, so it may be run on
Lunar Magic-saved ROMs to find what a hook leaves behind. Do not extend it to print
where in the ROM a write came from, or to dump ROM contents: that would be an
instruction trace or a copy of Lunar Magic's code.
"""
import argparse, collections, os, subprocess, sys, tempfile

WRAM = 0x20000
STACK = (0x0110, 0x0200)


def wram(kobo, rom, level):
    out = subprocess.run([kobo, 'level', 'wram', '-r', rom, level, '$7E0000', str(WRAM)],
                         capture_output=True, text=True)
    if out.returncode:
        return None
    data = bytearray()
    for line in out.stdout.splitlines():
        data += bytes.fromhex(line.split(':', 1)[1])
    # The stack holds return addresses, which are program counters: blank it, and
    # keep $0100-$010F, which the game and the tools use as variables.
    data[STACK[0]:STACK[1]] = bytes(STACK[1] - STACK[0])
    return bytes(data)


def video(kobo, rom, level):
    with tempfile.TemporaryDirectory() as d:
        out = subprocess.run([kobo, 'level', 'dump', '-r', rom, level, d], capture_output=True)
        if out.returncode:
            return None
        n = int(level, 16)
        read = lambda k: open(os.path.join(d, f'level_{n:03X}.{k}.bin'), 'rb').read()
        return {'vram': read('vram'), 'cgram': read('cgram')}


def reverted(a, b, spec):
    """b's image with the given LoROM ranges taken from a: an ablation. Headerless."""
    def load(p):
        d = open(p, 'rb').read()
        return d[512:] if len(d) % 0x8000 == 512 else d
    va, out = load(a), bytearray(load(b))
    for r in spec.split(','):
        s, _, e = r.partition('-')
        s = int(s.lstrip('$'), 16); e = int((e or s).lstrip('$'), 16) if e else s
        for x in range(s, e + 1):
            o = ((x >> 16) & 0x7F) * 0x8000 + (x & 0x7FFF)
            out[o] = va[o]
    return bytes(out)


def runs(a, b, gap=4):
    i, n, out = 0, min(len(a), len(b)), []
    while i < n:
        if a[i] == b[i]:
            i += 1
            continue
        j = last = i
        while j < n and j - last <= gap:
            if a[j] != b[j]:
                last = j
            j += 1
        out.append((i, last + 1))
        i = last + 1
    return out


def show(name, base, a, b, limit=24):
    for s, e in runs(a, b):
        cut = min(e, s + limit)
        more = ' ...' if e > cut else ''
        print(f'  {name} ${base + s:06X}+{e - s:<4} {a[s:cut].hex(" ")}{more}  ->  {b[s:cut].hex(" ")}{more}')


def main():
    p = argparse.ArgumentParser()
    p.add_argument('a'); p.add_argument('b'); p.add_argument('levels', nargs='+')
    p.add_argument('--kobo', default='kobo'); p.add_argument('--vram', action='store_true')
    p.add_argument('--summary', action='store_true')
    p.add_argument('--revert', help='SNES ranges (6-digit hex, START-END,...) copied from a '
                   'into a temporary copy of b, which then stands in for a')
    o = p.parse_args()
    levels = [f'{n:X}' for n in range(0x200)] if o.levels == ['all'] else o.levels
    if o.revert:
        tmp = tempfile.NamedTemporaryFile(suffix='.smc', delete=False)
        tmp.write(reverted(o.a, o.b, o.revert)); tmp.close()
        o.a = tmp.name
    count, loaded = collections.Counter(), 0
    for lv in levels:
        a, b = wram(o.kobo, o.a, lv), wram(o.kobo, o.b, lv)
        if a is None or b is None:
            print(f'level {lv}: failed to load in {"a" if a is None else "b"}', file=sys.stderr)
            continue
        loaded += 1
        if o.summary:
            for s, e in runs(a, b, gap=0):
                for x in range(s, e):
                    count[0x7E0000 + x] += 1
            continue
        print(f'level {lv}:')
        show('wram', 0x7E0000, a, b)
        if o.vram:
            va, vb = video(o.kobo, o.a, lv), video(o.kobo, o.b, lv)
            if va and vb:
                show('vram', 0, va['vram'], vb['vram'], 16)
                show('cgram', 0, va['cgram'], vb['cgram'], 16)
    if o.summary:
        print(f'{loaded} levels loaded in both; addresses that differ, with the number of levels:')
        addrs = sorted(count)
        i = 0
        while i < len(addrs):
            j = i
            while j + 1 < len(addrs) and addrs[j + 1] == addrs[j] + 1 and count[addrs[j + 1]] == count[addrs[i]]:
                j += 1
            print(f'  ${addrs[i]:06X}-${addrs[j]:06X}: {count[addrs[i]]}')
            i = j + 1


if __name__ == '__main__':
    main()
