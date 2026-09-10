"""Verify equipment-cache references and native synchronous/queued file reads.

Runs the supplied DLL in private Unicorn memory. File/pack I/O and events are
stubbed; the actual loader, queue dispatcher and mode-7 worker execute unchanged.
This checks CPU buffer boundaries and request sequencing, not an in-game session.
"""

import argparse
import re
import struct
from pathlib import Path

from capstone import CS_OP_IMM
from unicorn import UC_HOOK_CODE

from verify_native import (
    CODE, DATA, STOP, DISASM, cpu, sections, u32, reg,
    address, read_operand, write_operand,
)

BASE = 0x10000000
HEAP = 0x30000000
SLOT_SIZE = 0x20000
CANARY = b"\xa5" * 32
QUEUE = 0x1E866D40
SET_EVENT = CODE + 0x3000
WAIT_EVENT = CODE + 0x3010


def config():
    source = (Path(__file__).parents[1] / "src/equipment_cache.rs").read_text()
    rvas = re.search(r"const CACHE_RVAS:.*?=\s*\[(.*?)\];", source, re.S).group(1)
    bases = [BASE + int(value, 16) for value in re.findall(r"0x[0-9a-f]+", rvas)]
    references = []
    for item in re.findall(r"CacheReference\s*\{([^{}]+)\}", source, re.S):
        if "original: &[" not in item:
            continue
        number = lambda key: int(re.search(rf"\b{key}:\s*(0x[0-9a-f]+|\d+)", item).group(1), 0)
        original = re.search(r"original:\s*&\[([^]]+)\]", item).group(1)
        references.append((number("rva"), bytes(int(x, 16) for x in re.findall(r"0x[0-9a-f]+", original)),
                           number("operand_offset"), number("cache"), number("offset")))
    assert len(bases) == 14 and len(references) == 33
    return bases, references


def write32(uc, location, value):
    uc.mem_write(location, struct.pack("<I", value))


def read32(uc, location):
    return u32(uc.mem_read(location, 4))


def cstring(uc, location):
    result = bytearray()
    while byte := uc.mem_read(location + len(result), 1)[0]:
        result.append(byte)
        assert len(result) < 260
    return bytes(result)


def return_stub(uc, result=0, pop=0):
    sp = uc.reg_read(reg.UC_X86_REG_ESP)
    uc.reg_write(reg.UC_X86_REG_EIP, read32(uc, sp))
    uc.reg_write(reg.UC_X86_REG_ESP, sp + 4 + pop)
    uc.reg_write(reg.UC_X86_REG_EAX, result)


def mapped_client(data):
    uc = cpu()
    uc.mem_map(BASE, 0x0F11C000)
    for rva, size, offset in sections(data):
        if size:
            uc.mem_write(BASE + rva, data[offset:offset + size])
    uc.mem_map(HEAP, 0x2000000)
    return uc


def run(uc, entry, arguments=()):
    sp = uc.reg_read(reg.UC_X86_REG_ESP)
    uc.mem_write(sp, struct.pack("<" + "I" * (1 + len(arguments)), STOP, *arguments))
    saved = {r: uc.reg_read(r) for r in [reg.UC_X86_REG_EBX, reg.UC_X86_REG_ESI, reg.UC_X86_REG_EDI, reg.UC_X86_REG_EBP]}
    uc.emu_start(entry, STOP, count=1_000_000)
    assert uc.reg_read(reg.UC_X86_REG_EIP) == STOP
    assert uc.reg_read(reg.UC_X86_REG_ESP) == sp + 4
    for register, value in saved.items():
        assert uc.reg_read(register) == value
    uc.reg_write(reg.UC_X86_REG_ESP, sp)


def verify_references(data, bases, references):
    uc = mapped_client(data)
    for rva, original, operand, cache, offset in references:
        assert uc.mem_read(BASE + rva, len(original)) == original, hex(rva)
        assert u32(original, operand) == bases[cache] + offset, hex(rva)
        destination = HEAP + cache * 0x200000 + 32
        replacement = bytearray(original)
        struct.pack_into("<I", replacement, operand, destination + offset)
        instruction = next(DISASM.disasm(replacement, CODE))
        assert instruction.size == len(original), hex(rva)
        for register in [reg.UC_X86_REG_EAX, reg.UC_X86_REG_EDX, reg.UC_X86_REG_EBX, reg.UC_X86_REG_ECX]:
            uc.reg_write(register, 0x40)
        uc.mem_write(CODE, bytes(replacement))
        if instruction.mnemonic == "cmp":
            right = instruction.operands[1]
            expected = right.imm if right.type == CS_OP_IMM else read_operand(uc, instruction, right)
            write_operand(uc, instruction, instruction.operands[0], expected)
            uc.emu_start(CODE, CODE + len(original))
            assert uc.reg_read(reg.UC_X86_REG_EFLAGS) & 0x40, hex(rva)
            write_operand(uc, instruction, instruction.operands[0], expected ^ 1)
            uc.emu_start(CODE, CODE + len(original))
            assert not uc.reg_read(reg.UC_X86_REG_EFLAGS) & 0x40, hex(rva)
        elif instruction.mnemonic == "lea":
            expected = address(uc, instruction, instruction.operands[1])
            uc.emu_start(CODE, CODE + len(original))
            assert read_operand(uc, instruction, instruction.operands[0]) == expected
        else:
            assert instruction.mnemonic == "mov"
            source = instruction.operands[1]
            if source.type == CS_OP_IMM:
                expected = destination + offset
            else:
                expected = 0x12345678
                write_operand(uc, instruction, source, expected)
            uc.emu_start(CODE, CODE + len(original))
            assert read_operand(uc, instruction, instruction.operands[0]) == expected, hex(rva)
    print("PASS: 33 real consumer instructions relocate all 14 weapon/armor caches, including indexed JKR fields")


def verify_read(data, bases, cache, size, mode, old_buffer=False):
    uc = mapped_client(data)
    path = b"weapon\\we001.bin" if cache < 2 else b"parts\\m00\\m_body001.bin"
    queued = isinstance(mode, int)
    stored_path = b"dat\\" + path if queued else path
    payload = bytes((i % 251 for i in range(size)))
    length = max(SLOT_SIZE, size + len(stored_path) + 1)
    buffer = bases[cache] if old_buffer else HEAP + cache * 0x200000 + 32
    uc.mem_write(bases[cache] + SLOT_SIZE, CANARY)
    if not old_buffer:
        uc.mem_write(buffer - 32, CANARY)
        uc.mem_write(buffer + length, CANARY)
    uc.mem_write(DATA, stored_path + b"\0")
    for stub in [SET_EVENT, WAIT_EVENT]:
        uc.mem_write(stub, b"\xc3")
    write32(uc, 0x115D21A4, SET_EVENT)
    write32(uc, 0x115D21A0, WAIT_EVENT)
    writes = []

    def hook(emu, location, _size, _user):
        sp = emu.reg_read(reg.UC_X86_REG_ESP)
        arg = lambda i: read32(emu, sp + 4 + i * 4)
        if location == 0x1000AFF0:
            return_stub(emu, size)
        elif location == 0x1158CB60:
            return_stub(emu, 0 if mode == "pack" else 0xFFFFFFFF)
        elif location in (0x1158C940, 0x1158CA50):
            target = arg(1)
            filename = cstring(emu, arg(0))
            emu.mem_write(target, payload + filename + b"\0")
            writes.append(target)
            return_stub(emu, 1 if queued else 2)
        elif location == 0x1000AD00:
            assert arg(1) >= size
            emu.mem_write(arg(0), payload)
            writes.append(arg(0))
            return_stub(emu, 1)
        elif location == 0x1158F510:
            # Payload decoding is separately checked by verify_native.py.
            return_stub(emu, 1)
        elif location == SET_EVENT:
            return_stub(emu, 1, pop=4)
        elif location == WAIT_EVENT:
            return_stub(emu, 0, pop=8)

    for target in [0x1000AFF0, 0x1158CB60, 0x1158C940, 0x1158CA50, 0x1000AD00, 0x1158F510, SET_EVENT, WAIT_EVENT]:
        uc.hook_add(UC_HOOK_CODE, hook, begin=target, end=target)
    if queued:
        write32(uc, 0x1E866CE0, 1)
        write32(uc, 0x1E866CE4, 0)
        write32(uc, 0x1E866CE8, 1)
        uc.mem_write(QUEUE, struct.pack("<III", mode, buffer, 0) + stored_path + b"\0")
        run(uc, 0x115904C0)
        if mode == 7:
            assert read32(uc, QUEUE) == 6
            assert read32(uc, buffer + 4) == buffer
            assert cstring(uc, buffer + 12) == stored_path
            assert not writes, "mode 7 must wait for its worker"
            run(uc, 0x115903A0, [buffer, 0, DATA + 0x5000])
            run(uc, 0x115904C0)
        assert read32(uc, QUEUE) == 0
        assert read32(uc, 0x1E866CE4) == 1
    else:
        uc.reg_write(reg.UC_X86_REG_EAX, DATA)
        run(uc, 0x108E27D0, [buffer])
        assert uc.reg_read(reg.UC_X86_REG_EAX) == size + len(stored_path)
    assert writes == [buffer]
    assert uc.mem_read(buffer, size) == payload
    if mode != "loose":
        assert uc.mem_read(buffer + size, len(stored_path) + 1) == stored_path + b"\0"
    if old_buffer:
        assert uc.mem_read(bases[cache] + SLOT_SIZE, 32) != CANARY, "negative control did not reproduce the old overflow"
    else:
        assert uc.mem_read(buffer - 32, 32) == CANARY
        assert uc.mem_read(buffer + length, 32) == CANARY
        assert uc.mem_read(bases[cache] + SLOT_SIZE, 32) == CANARY


def verify_rejected_resource(data):
    uc = mapped_client(data)
    # Match INVALID_RESOURCE: ECD v4, zero-length path, incorrect filename tag.
    uc.mem_write(HEAP, b"ecd\x1a\x04" + b"\0" * 12)

    def crt(emu, location, _size, _user):
        sp = emu.reg_read(reg.UC_X86_REG_ESP)
        if location == 0x115B1AD3:  # _splitpath(empty, drive, dir, filename, ext)
            for index in range(1, 5):
                emu.mem_write(read32(emu, sp + 4 + index * 4), b"\0")
        elif location == 0x115B16DB:  # _makepath writes the empty checksum name
            emu.mem_write(read32(emu, sp + 4), b"\0")
        return_stub(emu)

    for target in [0x115B1AD3, 0x115B16DB, 0x115B18C4]:
        uc.hook_add(UC_HOOK_CODE, crt, begin=target, end=target)
    uc.reg_write(reg.UC_X86_REG_ESI, HEAP)
    run(uc, 0x1158F510, [0])
    assert uc.reg_read(reg.UC_X86_REG_EAX) == 0
    print("PASS: native ECD validation rejects the failed-load marker before model decoding")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("client", type=Path)
    args = parser.parse_args()
    data = args.client.read_bytes()
    bases, references = config()
    verify_references(data, bases, references)
    verify_rejected_resource(data)
    verify_read(data, bases, 1, 130 * 1024, "loose", old_buffer=True)
    print("PASS: the original 128-KiB weapon cache reproduces adjacent-memory corruption at 130 KiB")
    count = 0
    for cache in [0, 1, 2, 8]:
        modes = [2, 7] if cache in [0, 2] else ["loose", "pack"]
        for size in [SLOT_SIZE - 1, SLOT_SIZE, 130 * 1024, 1024 * 1024]:
            for mode in modes:
                verify_read(data, bases, cache, size, mode)
                count += 1
    print(f"PASS: {count} native weapon/armor reads preserve allocation and original-cache canaries")
    print("PASS: loose/pack synchronous loading, queued modes 2/7, request completion and trailing filename NUL")


if __name__ == "__main__":
    main()
