"""Execute the geometry instruction edits and compiled x86 ABI shims in Unicorn.

Development-only dependencies: unicorn, capstone. No Windows or running game is
required. The supplied DLL is read only; all execution uses private emulator RAM.
"""

import argparse
import re
import struct
from pathlib import Path

from capstone import Cs, CS_ARCH_X86, CS_MODE_32, CS_OP_MEM, CS_OP_REG
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn import x86_const as reg

CODE = 0x40000000
STOP = CODE + 0x1000
STUB = CODE + 0x2000
DATA = 0x50000000
STACK = 0x60000000
DISASM = Cs(CS_ARCH_X86, CS_MODE_32)
DISASM.detail = True


def u32(data, offset=0):
    return struct.unpack_from("<I", data, offset)[0]


def cpu():
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    for address, size in [(CODE, 0x10000), (DATA, 0x10000), (STACK, 0x10000)]:
        uc.mem_map(address, size)
    uc.mem_write(DATA, b"\xcc" * 0x10000)
    for name in ["EAX", "EBX", "ECX", "EDX", "ESI", "EDI", "EBP"]:
        uc.reg_write(getattr(reg, "UC_X86_REG_" + name), DATA + 0x800)
    uc.reg_write(reg.UC_X86_REG_ESP, STACK + 0x800)
    uc.reg_write(reg.UC_X86_REG_EFLAGS, 0x202)
    uc.mem_write(STUB, b"\xc3")
    return uc


def register(instruction, number):
    return getattr(reg, "UC_X86_REG_" + instruction.reg_name(number).upper())


def address(uc, instruction, operand):
    value = operand.mem.disp
    if operand.mem.base:
        value += uc.reg_read(register(instruction, operand.mem.base))
    if operand.mem.index:
        value += operand.mem.scale * uc.reg_read(register(instruction, operand.mem.index))
    return value & 0xFFFFFFFF


def read_operand(uc, instruction, operand):
    if operand.type == CS_OP_REG:
        return uc.reg_read(register(instruction, operand.reg))
    assert operand.type == CS_OP_MEM
    return int.from_bytes(uc.mem_read(address(uc, instruction, operand), operand.size), "little")


def write_operand(uc, instruction, operand, value):
    if operand.type == CS_OP_REG:
        uc.reg_write(register(instruction, operand.reg), value)
    else:
        uc.mem_write(address(uc, instruction, operand), value.to_bytes(operand.size, "little"))


def sections(data):
    pe = u32(data, 0x3C)
    assert data[pe:pe + 4] == b"PE\0\0"
    assert struct.unpack_from("<H", data, pe + 4)[0] == 0x14C
    count = struct.unpack_from("<H", data, pe + 6)[0]
    start = pe + 24 + struct.unpack_from("<H", data, pe + 20)[0]
    return [(u32(data, start + i * 40 + 12), u32(data, start + i * 40 + 16),
             u32(data, start + i * 40 + 20)) for i in range(count)]


def patches():
    text = (Path(__file__).parents[1] / "src" / "patches.rs").read_text()
    pattern = r"Patch\s*\{\s*rva:\s*(0x[0-9a-f]+),\s*original:\s*&\[([^]]*)\],\s*replacement:\s*&\[([^]]*)\]"
    for rva, old, new in re.findall(pattern, text):
        decode = lambda value: bytes(int(x, 16) for x in re.findall(r"0x[0-9a-f]+", value))
        yield int(rva, 16), decode(old), decode(new)


def verify_patches(data):
    spans = sections(data)
    count = 0
    for rva, old, new in patches():
        va, _, offset = next(s for s in spans if s[0] <= rva < s[0] + s[1])
        offset += rva - va
        assert data[offset:offset + len(old)] == old, hex(rva)
        assert len(old) == len(new)
        original = next(DISASM.disasm(old, CODE))
        replacement = next(DISASM.disasm(new, CODE))
        # Execute both selector comparison outcomes, including nonzero tags.
        values = [0, 17] if original.mnemonic == "cmp" else [0x10007]
        for value in values:
            uc = cpu()
            uc.mem_write(CODE, new)
            if original.mnemonic == "movzx":
                assert replacement.mnemonic == "mov"
                assert replacement.operands[1].size == 4
                assert replacement.operands[1].mem.disp == 2 * original.operands[1].mem.disp
                write_operand(uc, replacement, replacement.operands[1], value)
                uc.emu_start(CODE, CODE + len(new))
                assert read_operand(uc, replacement, replacement.operands[0]) == value, hex(rva)
            elif original.mnemonic == "add":
                assert replacement.operands[1].imm == original.operands[1].imm * 2
                before = DATA + 0x3000
                write_operand(uc, replacement, replacement.operands[0], before)
                uc.emu_start(CODE, CODE + len(new))
                assert read_operand(uc, replacement, replacement.operands[0]) == before + replacement.operands[1].imm, hex(rva)
            elif original.mnemonic == "mov":
                assert replacement.operands[1].mem.disp == 2 * original.operands[1].mem.disp
                write_operand(uc, replacement, replacement.operands[1], 17)
                uc.emu_start(CODE, CODE + len(new))
                assert read_operand(uc, replacement, replacement.operands[0]) == 17, hex(rva)
            elif original.mnemonic == "cmp":
                assert replacement.operands[0].mem.disp == 2 * original.operands[0].mem.disp
                write_operand(uc, replacement, replacement.operands[0], value)
                uc.emu_start(CODE, CODE + len(new))
                assert bool(uc.reg_read(reg.UC_X86_REG_EFLAGS) & 0x40) == (value == 0), hex(rva)
            else:
                raise AssertionError((hex(rva), original.mnemonic))
        count += 1
    assert count == 94
    print(f"PASS: {count} native instruction edits match the DLL and execute correctly")


def coff_functions(path):
    """Read named function sections from the actual rustc i686 COFF object."""
    data = path.read_bytes()
    machine, count, _, table, symbols, optional, _ = struct.unpack_from("<HHIIIHH", data)
    assert machine == 0x14C and optional == 0, "expected an ordinary i686 COFF object"
    spans = []
    for i in range(count):
        start = 20 + i * 40
        spans.append((u32(data, start + 16), u32(data, start + 20)))
    strings = table + symbols * 18
    index = 0
    result = {}
    while index < symbols:
        start = table + index * 18
        name, value, section, _, _, auxiliaries = struct.unpack_from("<8sIhHBB", data, start)
        if name[:4] == b"\0" * 4:
            offset = strings + u32(name, 4)
            name = data[offset:data.index(b"\0", offset)]
        name = name.rstrip(b"\0").decode(errors="replace")
        if section > 0 and "mhf_geometry" in name and "abi" in name:
            for function in ["load_detour", "build_detour", "load_original", "build_original", "schedule", "convert_vertices", "source_query"]:
                if function in name:
                    size, offset = spans[section - 1]
                    result[function] = data[offset + value:offset + size]
        index += 1 + auxiliaries
    assert len(result) == 7, result.keys()
    return result


def verify_abi(path):
    for name, code in coff_functions(path).items():
        uc = cpu()
        code = bytearray(code)
        if name.endswith("detour"):
            calls = [i for i in DISASM.disasm(code, CODE) if i.mnemonic == "call"]
            assert len(calls) == 1 and calls[0].bytes[0] == 0xE8
            offset = calls[0].address - CODE
            struct.pack_into("<i", code, offset + 1, STUB - (CODE + offset + 5))
            arguments = [0x12345678, 0x22334455]
        elif name == "convert_vertices":
            arguments = [STUB, DATA + 0x2000, DATA + 0x3000, 0x152, 70001, 0x35]
        elif name == "schedule":
            arguments = [STUB, 0x12345678, DATA + 0x3000]
        elif name == "source_query":
            arguments = [STUB, DATA + 0x3000]
        else:
            arguments = [STUB, 70001 if name == "load_original" else DATA + 0x2000, DATA + 0x3000, DATA + 0x4000]
        sp = uc.reg_read(reg.UC_X86_REG_ESP)
        uc.mem_write(sp, struct.pack("<" + "I" * (1 + len(arguments)), STOP, *arguments))
        uc.mem_write(CODE, bytes(code))
        preserved = {r: uc.reg_read(r) for r in [reg.UC_X86_REG_EBX, reg.UC_X86_REG_ESI, reg.UC_X86_REG_EDI, reg.UC_X86_REG_EBP]}
        calls = []

        def callee(emu, _address, _size, _data):
            current = emu.reg_read(reg.UC_X86_REG_ESP)
            stack = lambda i: u32(emu.mem_read(current + 4 + 4 * i, 4))
            if name.endswith("detour"):
                frame = stack(0)
                saved_sp = u32(emu.mem_read(frame + 12, 4))
                assert u32(emu.mem_read(saved_sp + 8, 4)) == arguments[0]
                emu.mem_write(frame + 28, struct.pack("<I", 0x76543210))
            elif name == "load_original":
                assert emu.reg_read(reg.UC_X86_REG_EAX) == arguments[1]
                assert [stack(0), stack(1)] == arguments[2:]
            elif name == "build_original":
                assert emu.reg_read(reg.UC_X86_REG_ECX) == arguments[1]
                assert [stack(0), stack(1)] == arguments[2:]
            elif name == "convert_vertices":
                assert [emu.reg_read(r) for r in [reg.UC_X86_REG_EAX, reg.UC_X86_REG_ECX, reg.UC_X86_REG_EDI]] == arguments[1:4]
                assert [stack(0), stack(1)] == arguments[4:]
            elif name == "schedule":
                assert [emu.reg_read(r) for r in [reg.UC_X86_REG_EDI, reg.UC_X86_REG_ESI]] == arguments[1:]
            elif name == "source_query":
                assert emu.reg_read(reg.UC_X86_REG_EAX) == arguments[1]
            emu.reg_write(reg.UC_X86_REG_EAX, 0x76543210)
            emu.reg_write(reg.UC_X86_REG_ECX, 0x11223344)
            emu.reg_write(reg.UC_X86_REG_EDX, 0x22334455)
            calls.append(True)

        uc.hook_add(UC_HOOK_CODE, callee, begin=STUB, end=STUB)
        uc.emu_start(CODE, STOP, count=1000)
        assert len(calls) == 1, name
        assert uc.reg_read(reg.UC_X86_REG_EAX) == 0x76543210, name
        assert uc.reg_read(reg.UC_X86_REG_ESP) == sp + 4, name
        for r, value in preserved.items():
            assert uc.reg_read(r) == value, (name, r)
        if name.endswith("detour"):
            assert uc.reg_read(reg.UC_X86_REG_EFLAGS) == 0x202
    print("PASS: 7 compiled x86 adapters preserve the native argument, register and stack ABI")


def block(kind, count, payload):
    return struct.pack("<III", kind, count, len(payload) + 12) + payload


def converter_fixture(count, skinned):
    indices = [0, 1, 2] if count == 3 else [65535, 65536, count - 1]
    faces = block(5, 1, block(0x30000, 1, struct.pack("<IIII", 3, *indices)))
    positions = b"".join(struct.pack("<fff", float(i), 0.0, 0.0) for i in range(count))
    children = [faces, block(0x70000, count, positions),
                block(0x80000, count, struct.pack("<fff", 0.0, 1.0, 0.0) * count),
                block(0xA0000, count, struct.pack("<ff", 0.0, 0.0) * count),
                block(0xB0000, count, struct.pack("<ffff", 255.0, 255.0, 255.0, 255.0) * count),
                block(0x60000, 1, struct.pack("<I", 0))]
    if skinned:
        children.append(block(0xC0000, count, struct.pack("<IIf", 1, 0, 1.0) * count))
    return block(1, 1, block(2, 1, block(4, len(children), b"".join(children)))), indices


def verify_converters(data):
    """Run the game's real pre-upload conversion, including its CRT allocator ABI."""
    for count in [3, 70001]:
        for skinned in [False, True]:
            file, indices = converter_fixture(count, skinned=skinned)
            uc = cpu()
            uc.mem_map(0x10000000, 0x0F11C000)
            for rva, size, offset in sections(data):
                if size:
                    uc.mem_write(0x10000000 + rva, data[offset:offset + size])
            uc.mem_map(0x30000000, 0x08000000)
            uc.mem_write(0x30000000, file)
            source = 0x31000000
            allocations = []
            cursor = 0x32000000

            def malloc(emu, _address, _size, _data):
                nonlocal cursor
                sp = emu.reg_read(reg.UC_X86_REG_ESP)
                size = u32(emu.mem_read(sp + 4, 4))
                pointer = cursor
                cursor = (cursor + size + 63) & ~15
                assert cursor < 0x38000000
                emu.mem_write(pointer + size, b"\xa5" * 32)
                allocations.append((pointer, size))
                emu.reg_write(reg.UC_X86_REG_EAX, pointer)
                emu.reg_write(reg.UC_X86_REG_EIP, u32(emu.mem_read(sp, 4)))
                emu.reg_write(reg.UC_X86_REG_ESP, sp + 4)

            uc.hook_add(UC_HOOK_CODE, malloc, begin=0x115AB67E, end=0x115AB67E)
            sp = uc.reg_read(reg.UC_X86_REG_ESP)
            uc.mem_write(sp, struct.pack("<III", STOP, source, 0x30000000))
            uc.reg_write(reg.UC_X86_REG_EAX, 0)
            uc.emu_start(0x10002AF0, STOP, count=50_000_000)
            assert uc.reg_read(reg.UC_X86_REG_EIP) == STOP
            assert uc.reg_read(reg.UC_X86_REG_EAX) == 1
            fields = struct.unpack("<8I", uc.mem_read(source, 32))
            assert fields[1] == 1 and fields[5] == count
            # This verifies the native intermediate that load_indices replaces.
            old_indices = struct.unpack("<" + "H" * (fields[7] // 2), uc.mem_read(fields[2], fields[7]))
            assert list(old_indices[-3:]) == [i & 65535 for i in indices]
            for pointer, size in allocations:
                assert uc.mem_read(pointer + size, 32) == b"\xa5" * 32, "native converter allocation overrun"
    print("PASS: real native static/skinned converters preserve the expected ABI at 3 and 70001 vertices")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("client", type=Path)
    parser.add_argument("--object", type=Path, help="mhf_geometry-*.o produced with rustc --emit=obj -C codegen-units=1")
    parser.add_argument("--converters", action="store_true", help="also execute the real native static/skinned converters")
    args = parser.parse_args()
    data = args.client.read_bytes()
    verify_patches(data)
    if args.object:
        verify_abi(args.object)
    if args.converters:
        verify_converters(data)


if __name__ == "__main__":
    main()
