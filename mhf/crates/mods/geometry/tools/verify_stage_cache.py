"""Verify moving stage caches against a read-only DLL's native readers and decoder.

Requires the development-only Unicorn/Capstone environment of verify_native.py.
Allocation/dispatch, file/pack I/O, ECD validation and CRT memory operations are substituted.
Actual native readers, decoder call instructions, PAC routing, raw-JKR decoding,
pointer-cell consumer edits, scene initialization and motion LZ decoding execute
in private emulator memory.
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
    rvas = re.search(r"const PATCH_RVAS:.*?=\s*\[([^]]+)\]", source).group(1)
    original = re.search(r"const ORIGINAL:.*?=\s*\[([^]]+)\]", source).group(1)
    old = bytes(int(value, 16) for value in re.findall(r"0x[0-9a-f]+", original))
    for rva in re.findall(r"0x[0-9a-f]+", rvas):
        new = b"\x8b\x0d" + struct.pack("<I", FSKL_CELL)
        assert len(old) == len(new)
        edits.append((int(rva, 16), old, new))
    assert len(edits) == 4
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
    for rva, old, new in edits:
        assert uc.mem_read(BASE + rva, len(old)) == old, hex(rva)
        uc.mem_write(BASE + rva, new)
    return uc


def scene_reset(uc):
    cleared = []

    def memset(emu, _address, _size, _data):
        sp = emu.reg_read(reg.UC_X86_REG_ESP)
        target, value, size = struct.unpack("<III", emu.mem_read(sp + 4, 12))
        emu.mem_write(target, bytes([value & 0xFF]) * size)
        cleared.append((target, size))
        return_stub(emu, target)

    hook = uc.hook_add(UC_HOOK_CODE, memset, begin=0x115B4150, end=0x115B4150)
    sp = uc.reg_read(reg.UC_X86_REG_ESP)
    try:
        # Run the global clear in 10B70380 before the native arena assignments.
        # Starting at 108E33BD missed that clear and accepted unsafe NOP edits.
        uc.emu_start(0x108E33A0, 0x108E349C, count=100_000)
        assert uc.reg_read(reg.UC_X86_REG_EIP) == 0x108E349C
        assert cleared == [(0x1ED52860, 0x128), (0x11E86340, 0xBD16400)]
    finally:
        uc.reg_write(reg.UC_X86_REG_ESP, sp)
        uc.hook_del(hook)


def verify_motion_after_reset(uc):
    # Ten empty animation banks let the actual 1089F700 caller and 108FD1D0
    # unpack a literal-only LZ stream without allocating compiled MOT objects.
    payload = bytes(10 * 8)
    packed = b"".join(b"\0" + payload[start:start + 8]
                      for start in range(0, len(payload), 8))
    source = b"JKR\x1a" + struct.pack("<HHII", 0x0108, 3, 16, len(payload)) + packed
    uc.mem_write(SOURCE, source)
    write32(uc, 0x1ED528B8, SOURCE)
    destination = read32(uc, 0x1ED528BC)
    if destination:
        uc.mem_write(destination, b"\xcc" * len(payload))
        uc.mem_write(destination - len(CANARY), CANARY)
        uc.mem_write(destination + len(payload), CANARY)
    sp = uc.reg_read(reg.UC_X86_REG_ESP)
    try:
        # Includes the live BC load, four arguments, CALL and stack cleanup.
        # A zero BC reproduces the reported write at 1158F1CC.
        uc.emu_start(0x1089F722, 0x1089F742, count=100_000)
        assert uc.reg_read(reg.UC_X86_REG_EIP) == 0x1089F742
        assert uc.reg_read(reg.UC_X86_REG_ESP) == sp
        assert uc.mem_read(destination, len(payload)) == payload
        assert uc.mem_read(destination - len(CANARY), len(CANARY)) == CANARY
        assert uc.mem_read(destination + len(payload), len(CANARY)) == CANARY
    finally:
        uc.reg_write(reg.UC_X86_REG_ESP, sp)


def verify_reset(uc):
    for raw, decoded, fskl in [
        (0, 0, FSKL),
        (RAW, OLD_DECODED, OLD_DECODED + 0x02000000),
        (LARGE_RAW, FMOD, FSKL),
    ]:
        write32(uc, 0x1ED528BC, raw)
        write32(uc, 0x1ED528D8, decoded)
        write32(uc, FSKL_CELL, fskl)
        scene_reset(uc)
        assert read32(uc, 0x1ED528BC) == 0x1C404740
        assert read32(uc, 0x1ED528D8) == 0x12B04740
        assert read32(uc, FSKL_CELL) == fskl
        for alias in [0x1ED528D4, 0x1ED528DC, 0x1ED528E0]:
            assert read32(uc, alias) == 0x12B04740, hex(alias)
        verify_motion_after_reset(uc)
    print("PASS: native scene clear/reset restores BC/D8 and D4/DC/E0; actual motion LZ decode succeeds after initial and repeated resets")


def verify_consumers(uc, edits):
    count = 0
    for rva, _old, new in edits:
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
    size = 65 * 1024 * 1024 + 12
    payload = b"\x59" * size
    writes = []
    # Model reclamation of the former allocation. Any native dereference of its
    # address now faults, including after the reader returns to the stage loader.
    uc.mem_unmap(RAW, 0x01000000)
    write32(uc, 0x1ED528BC, LARGE_RAW)

    def io(emu, location, _size, _user):
        sp = emu.reg_read(reg.UC_X86_REG_ESP)
        if location in [0x1000AFF0, 0x1000B0B0]:
            return_stub(emu, size)
        elif location == 0x1158CB60:
            return_stub(emu, 0)
        elif location == 0x1158C940:
            target = read32(emu, sp + 8)
            assert target == read32(emu, 0x1ED528BC) == LARGE_RAW
            emu.mem_write(target, contents)
            writes.append(target)
            return_stub(emu, 2)
        else:
            assert location == 0x1158F510
            return_stub(emu, 1)

    hooks = [uc.hook_add(UC_HOOK_CODE, io, begin=address, end=address)
             for address in [0x1000AFF0, 0x1000B0B0, 0x1158CB60, 0x1158C940, 0x1158F510]]
    sp = uc.reg_read(reg.UC_X86_REG_ESP)
    for call, path in [(0x1089F121, b"stage-hd\\st244.pac"),
                       (0x1089F14F, b"stage\\st244.pac")]:
        contents = payload + path + b"\0"
        uc.mem_write(DATA, path + b"\0")
        uc.mem_write(LARGE_RAW + len(contents), CANARY)
        uc.reg_write(reg.UC_X86_REG_ESP, sp)
        write32(uc, sp, LARGE_RAW)
        uc.reg_write(reg.UC_X86_REG_EAX, DATA)
        uc.reg_write(reg.UC_X86_REG_EBX, RAW)
        # Execute both real reader CALLs with the dispatch-selected buffer.
        uc.emu_start(call, call + 5, count=100_000)
        assert uc.reg_read(reg.UC_X86_REG_ESP) == sp
        assert uc.reg_read(reg.UC_X86_REG_EAX) == size + len(path)
        assert uc.mem_read(LARGE_RAW, len(contents)) == contents
        assert uc.mem_read(LARGE_RAW + len(contents), len(CANARY)) == CANARY
        # The successful read branches to 1089F172 and reloads BC into EBX.
        uc.emu_start(call + 5, 0x1089F178, count=100)
        assert uc.reg_read(reg.UC_X86_REG_EIP) == 0x1089F178
        assert uc.reg_read(reg.UC_X86_REG_EBX) == LARGE_RAW
        assert uc.reg_read(reg.UC_X86_REG_ESP) == sp + 4
    assert writes == [LARGE_RAW, LARGE_RAW]
    uc.reg_write(reg.UC_X86_REG_ESP, sp)
    for hook in hooks:
        uc.hook_del(hook)
    print("PASS: real HD/SD reads and post-read routing use a moved >64-MiB PAC with the old buffer unmapped; filename/NUL preserved")


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
