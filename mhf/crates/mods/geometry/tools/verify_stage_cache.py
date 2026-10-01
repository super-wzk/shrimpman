"""Verify moving stage caches against a read-only DLL's native readers and decoder.

Requires the development-only Unicorn/Capstone environment of verify_native.py.
Allocation/dispatch, file/pack I/O, ECD validation and CRT memcpy are substituted.
Actual native readers, decoder call instructions, PAC routing, raw-JKR decoding,
pointer-cell consumer edits and reset code execute in private emulator memory.
This verifies native routing/ABI boundaries, not Rust allocation or game visuals.
Dynamic destinations are chosen by a Python dispatch stub; Rust buffer growth
and campaign preallocation are not executed by this script.
"""

import argparse
import re
import struct
from pathlib import Path

from unicorn import UC_HOOK_CODE

from verify_equipment_cache import read32, return_stub, write32
from verify_native import DATA, STOP, cpu, reg, sections

BASE = 0x10000000
RAW = 0x30000000
OLD_DECODED = 0x34000000
LARGE_RAW = 0x45000000
FMOD = 0x51000000
FSKL = 0x55000000
SOURCE = 0x70000000
FSKL_CELL = DATA + 0x4000
CANARY = b"\xa5" * 32
PRODUCERS = [0x104114AE, 0x1060E0AC, 0x1089F194, 0x1089F395]
DECODER = 0x108DF4B0


def config():
    source = (Path(__file__).parents[1] / "src/stage_cache.rs").read_text()
    edits = []
    for rva, original, kind in re.findall(
            r"Patch\s*\{\s*rva:\s*(0x[0-9a-f]+),\s*original:\s*&\[([^]]+)\],\s*edit:\s*Edit::(\w+)",
            source):
        old = bytes(int(value, 16) for value in re.findall(r"0x[0-9a-f]+", original))
        if kind in ["RawReset", "DecodedReset"]:
            new = b"\x90" * len(old)
        else:
            assert kind == "FsklPointer"
            new = b"\x8b\x0d" + struct.pack("<I", FSKL_CELL)
        assert len(old) == len(new)
        edits.append((int(rva, 16), old, new, kind))
    assert len(edits) == 6
    callers = re.search(r"const DECODE_CALLERS:.*?=\s*\[([^]]+)\]", source).group(1)
    callers = [BASE + int(value, 16) for value in re.findall(r"0x[0-9a-f]+", callers)]
    assert len(callers) == 4
    return edits, callers


def mapped_client(data, edits):
    uc = cpu()
    uc.mem_map(BASE, 0x0F11C000)
    for rva, size, offset in sections(data):
        if size:
            uc.mem_write(BASE + rva, data[offset:offset + size])
    for address, size in [
        (RAW, 0x01000000), (OLD_DECODED, 0x02800000),
        (LARGE_RAW, 0x09000000), (FMOD - 0x1000, 0x03001000),
        (FSKL - 0x1000, 0x01001000), (SOURCE, 0x08000000),
    ]:
        uc.mem_map(address, size)
    assert uc.mem_read(0x108E33BD, 5) == b"\xb8\x40\x47\xb0\x12"
    for rva, old, new, _kind in edits:
        assert uc.mem_read(BASE + rva, len(old)) == old, hex(rva)
        uc.mem_write(BASE + rva, new)
    return uc


def verify_reset(uc):
    for raw, decoded, fskl in [
        (RAW, OLD_DECODED, OLD_DECODED + 0x02000000),
        (LARGE_RAW, FMOD, FSKL),
    ]:
        write32(uc, 0x1ED528BC, raw)
        write32(uc, 0x1ED528D8, decoded)
        write32(uc, FSKL_CELL, fskl)
        # Execute the arena assignment and complete native pointer-reset span.
        uc.emu_start(0x108E33BD, 0x108E33C2)
        uc.emu_start(0x108E33CD, 0x108E349C)
        assert read32(uc, 0x1ED528BC) == raw
        assert read32(uc, 0x1ED528D8) == decoded
        assert read32(uc, FSKL_CELL) == fskl
        for alias in [0x1ED528D4, 0x1ED528DC, 0x1ED528E0]:
            assert read32(uc, alias) == 0x12B04740, hex(alias)
    print("PASS: real native resets retain moved BC/D8 and preserve D4/DC/E0")


def verify_consumers(uc, edits):
    count = 0
    for rva, _old, new, kind in edits:
        if kind != "FsklPointer":
            continue
        for fmod, fskl in [(OLD_DECODED, FSKL), (FMOD, FSKL + 0x800000)]:
            write32(uc, FSKL_CELL, fskl)
            uc.reg_write(reg.UC_X86_REG_EAX, fmod)
            uc.emu_start(BASE + rva, BASE + rva + len(new))
            assert uc.reg_read(reg.UC_X86_REG_ECX) == fskl, hex(rva)
            assert uc.reg_read(reg.UC_X86_REG_EAX) == fmod
            count += 1
    assert count == 8
    print("PASS: four actual FSKL consumer MOVs follow both pointer-cell values independently of EAX")


def raw_jkr(payload):
    return b"JKR\x1a" + struct.pack("<HHII", 0x0108, 0, 16, len(payload)) + payload


def pair_fixture(uc, fmod_size, fskl_size, jkr):
    fmod = (b"\x51\x62\x73\x84" * ((fmod_size + 3) // 4))[:fmod_size]
    fskl = (b"\x95\xa6\xb7\xc8" * ((fskl_size + 3) // 4))[:fskl_size]
    packed_fmod, packed_fskl = (raw_jkr(fmod), raw_jkr(fskl)) if jkr else (fmod, fskl)
    header = struct.pack("<IIIII", 2, 20, len(packed_fmod), 20 + len(packed_fmod), len(packed_fskl))
    uc.mem_write(SOURCE, header + packed_fmod + packed_fskl)
    return fmod, fskl


def verify_decoder(uc, callers):
    copies = []
    dispatches = []
    destinations = None

    def memcpy(emu, _address, _size, _data):
        sp = emu.reg_read(reg.UC_X86_REG_ESP)
        destination, source, size = struct.unpack("<III", emu.mem_read(sp + 4, 12))
        emu.mem_write(destination, bytes(emu.mem_read(source, size)))
        return_stub(emu, destination)
        copies.append((destination, size))

    def dispatch(emu, _address, _size, _data):
        caller = read32(emu, emu.reg_read(reg.UC_X86_REG_ESP))
        if caller not in callers:
            return
        fmod, fskl = destinations
        assert emu.reg_read(reg.UC_X86_REG_ESI) == SOURCE
        # Stand in for the Rust allocation/dispatch layer, then run the actual
        # decoder with its verified EAX/EDI/ESI register arguments.
        write32(emu, 0x1ED528D8, fmod)
        write32(emu, FSKL_CELL, fskl)
        emu.reg_write(reg.UC_X86_REG_EAX, fmod)
        emu.reg_write(reg.UC_X86_REG_EDI, fskl)
        dispatches.append(caller)

    memcpy_hook = uc.hook_add(UC_HOOK_CODE, memcpy, begin=0x115B49A0, end=0x115B49A0)
    dispatch_hook = uc.hook_add(UC_HOOK_CODE, dispatch, begin=DECODER, end=DECODER)
    sp = uc.reg_read(reg.UC_X86_REG_ESP)
    for index, (producer, caller) in enumerate(zip(PRODUCERS, callers)):
        call = caller - 5
        instruction = bytes(uc.mem_read(call, 5))
        assert instruction[0] == 0xE8
        assert call + 5 + struct.unpack("<i", instruction[1:])[0] == DECODER
        assert uc.mem_read(producer, 6) == b"\x8d\xb8\x00\x00\x80\x00"
        large = index >= 2
        fmod_size = 33 * 1024 * 1024 + 12 if large else 1024
        fskl_size = 5 * 1024 * 1024 + 12 if large else 2048
        fmod, fskl = pair_fixture(uc, fmod_size, fskl_size, jkr=index == 1)
        destinations = (FMOD + index * 0x1000, FSKL + index * 0x1000)
        for address, payload in zip(destinations, [fmod, fskl]):
            uc.mem_write(address - 32, CANARY)
            uc.mem_write(address + len(payload), CANARY)
        uc.mem_write(OLD_DECODED, CANARY)
        uc.mem_write(OLD_DECODED + 0x02000000, CANARY)
        uc.reg_write(reg.UC_X86_REG_EAX, OLD_DECODED)
        uc.reg_write(reg.UC_X86_REG_ESI, SOURCE)
        uc.emu_start(producer, producer + 6)
        assert uc.reg_read(reg.UC_X86_REG_EDI) == OLD_DECODED + 0x800000
        uc.emu_start(call, caller, count=100_000_000)
        assert uc.reg_read(reg.UC_X86_REG_EIP) == caller, (index, hex(uc.reg_read(reg.UC_X86_REG_EIP)))
        assert uc.reg_read(reg.UC_X86_REG_ESP) == sp
        assert dispatches[-1] == caller
        for address, payload in zip(destinations, [fmod, fskl]):
            assert uc.mem_read(address, len(payload)) == payload
            assert uc.mem_read(address - 32, 32) == CANARY
            assert uc.mem_read(address + len(payload), 32) == CANARY
        assert uc.mem_read(OLD_DECODED, 32) == CANARY
        assert uc.mem_read(OLD_DECODED + 0x02000000, 32) == CANARY
        assert read32(uc, 0x1ED528D8) == destinations[0]
        assert read32(uc, FSKL_CELL) == destinations[1]
    assert dispatches == callers
    assert copies == [(FMOD, 1024), (FSKL, 2048),
                      (FMOD + 0x2000, 33 * 1024 * 1024 + 12),
                      (FSKL + 0x2000, 5 * 1024 * 1024 + 12),
                      (FMOD + 0x3000, 33 * 1024 * 1024 + 12),
                      (FSKL + 0x3000, 5 * 1024 * 1024 + 12)]
    # An unrelated decoder caller must retain its own supplied destinations.
    fmod, fskl = pair_fixture(uc, 32, 64, jkr=False)
    previous = (read32(uc, 0x1ED528D8), read32(uc, FSKL_CELL))
    uc.mem_write(sp, struct.pack("<I", STOP))
    uc.reg_write(reg.UC_X86_REG_EAX, OLD_DECODED)
    uc.reg_write(reg.UC_X86_REG_EDI, OLD_DECODED + 0x800000)
    uc.reg_write(reg.UC_X86_REG_ESI, SOURCE)
    uc.emu_start(DECODER, STOP, count=100_000)
    assert uc.mem_read(OLD_DECODED, len(fmod)) == fmod
    assert uc.mem_read(OLD_DECODED + 0x800000, len(fskl)) == fskl
    assert (read32(uc, 0x1ED528D8), read32(uc, FSKL_CELL)) == previous
    assert dispatches == callers
    uc.reg_write(reg.UC_X86_REG_ESP, sp)
    uc.hook_del(dispatch_hook)
    uc.hook_del(memcpy_hook)
    print("PASS: four real decoder CALLs use dispatch-selected moving buffers; 33-MiB FMOD / 5-MiB FSKL, raw-JKR and non-stage routing checked")


def verify_raw_reader(uc):
    path = b"stage-hd\\st244.pac"
    size = 65 * 1024 * 1024 + 12
    payload = b"\x59" * size
    writes = []
    uc.mem_write(DATA, path + b"\0")
    uc.mem_write(RAW, CANARY)
    uc.mem_write(LARGE_RAW + size + len(path) + 1, CANARY)
    write32(uc, 0x1ED528BC, LARGE_RAW)

    def io(emu, location, _size, _user):
        sp = emu.reg_read(reg.UC_X86_REG_ESP)
        if location == 0x1000B0B0:
            return_stub(emu, size)
        elif location == 0x1158CB60:
            return_stub(emu, 0)
        elif location == 0x1158C940:
            target = read32(emu, sp + 8)
            assert target == read32(emu, 0x1ED528BC) == LARGE_RAW
            emu.mem_write(target, payload + path + b"\0")
            writes.append(target)
            return_stub(emu, 2)
        else:
            assert location == 0x1158F510
            return_stub(emu, 1)

    hooks = [uc.hook_add(UC_HOOK_CODE, io, begin=address, end=address)
             for address in [0x1000B0B0, 0x1158CB60, 0x1158C940, 0x1158F510]]
    sp = uc.reg_read(reg.UC_X86_REG_ESP)
    uc.mem_write(sp, struct.pack("<I", LARGE_RAW))
    uc.reg_write(reg.UC_X86_REG_EAX, DATA)
    # Execute the real stage HD reader call with the dispatch-selected buffer.
    uc.emu_start(0x1089F121, 0x1089F126, count=100_000)
    assert uc.reg_read(reg.UC_X86_REG_ESP) == sp
    assert uc.reg_read(reg.UC_X86_REG_EAX) == size + len(path)
    assert writes == [LARGE_RAW]
    assert uc.mem_read(LARGE_RAW, size) == payload
    assert uc.mem_read(LARGE_RAW + size, len(path) + 1) == path + b"\0"
    assert uc.mem_read(LARGE_RAW + size + len(path) + 1, 32) == CANARY
    assert uc.mem_read(RAW, 32) == CANARY
    for hook in hooks:
        uc.hook_del(hook)
    print("PASS: real stage HD reader CALL accepts a moved >64-MiB PAC buffer and retains trailing filename/NUL semantics")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("client", type=Path)
    args = parser.parse_args()
    edits, callers = config()
    uc = mapped_client(args.client.read_bytes(), edits)
    verify_reset(uc)
    verify_consumers(uc, edits)
    verify_decoder(uc, callers)
    verify_raw_reader(uc)


if __name__ == "__main__":
    main()
