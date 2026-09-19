"""Execute the monster ID checks from a real client DLL in Unicorn.

Development-only dependencies: capstone, unicorn. The supplied DLL is read only;
both the original and patched code execute exclusively in private emulator RAM.
This verifies the eight checks, not the separately supplied monster tables/assets.
"""

import argparse
import hashlib
import re
import struct
from dataclasses import dataclass
from pathlib import Path

from capstone import Cs, CS_ARCH_X86, CS_MODE_32, CS_OP_IMM
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn import x86_const as reg

CODE = 0x40000000
ACTOR = 0x50000000
SPAWN = ACTOR + 0x2000
STACK = 0x60000000
SP = STACK + 0x2000
BP = STACK + 0x1000
DISASM = Cs(CS_ARCH_X86, CS_MODE_32)
DISASM.detail = True


@dataclass(frozen=True)
class Check:
    name: str
    patch: int
    start: int
    stop: int
    recovery: int
    result_register: int | None = None


# Each span is real contiguous code ending at the branch merge, before any
# native table lookup/call. Relative branches remain valid when copied to CODE.
CHECKS = (
    Check("creation", 0x00AAA45A, 0x00AAA441, 0x00AAA469, 0x00AAA464, reg.UC_X86_REG_EAX),
    Check("initialization", 0x0086E494, 0x0086E494, 0x0086E49F, 0x0086E49B),
    Check("update", 0x0086E8C5, 0x0086E8C5, 0x0086E8CE, 0x0086E8CB),
    Check("action selection", 0x0086A662, 0x0086A662, 0x0086A66C, 0x0086A668),
    Check("action dispatch", 0x0086E95B, 0x0086E95B, 0x0086E965, 0x0086E961),
    Check("model parameters", 0x008FD3A3, 0x008FD3A3, 0x008FD3B5, 0x008FD3B0, reg.UC_X86_REG_EDX),
    Check("sound", 0x00B4698C, 0x00B46989, 0x00B469A4, 0x00B4699F, reg.UC_X86_REG_EAX),
    Check("additional sound", 0x00B47849, 0x00B47846, 0x00B47858, 0x00B47853, reg.UC_X86_REG_EAX),
)


def u32(data, offset):
    return struct.unpack_from("<I", data, offset)[0]


def sections(data):
    pe = u32(data, 0x3C)
    assert data[:2] == b"MZ" and data[pe:pe + 4] == b"PE\0\0", "expected a PE DLL"
    assert struct.unpack_from("<H", data, pe + 4)[0] == 0x14C, "expected an i386 DLL"
    count = struct.unpack_from("<H", data, pe + 6)[0]
    start = pe + 24 + struct.unpack_from("<H", data, pe + 20)[0]
    return [(u32(data, start + i * 40 + 12), u32(data, start + i * 40 + 16),
             u32(data, start + i * 40 + 20)) for i in range(count)]


def native_bytes(data, spans, rva, size):
    for address, raw_size, offset in spans:
        if address <= rva and rva + size <= address + raw_size:
            result = data[offset + rva - address:offset + rva - address + size]
            assert len(result) == size, f"truncated DLL at {rva:#x}"
            return result
    raise AssertionError(f"RVA {rva:#x} is outside the DLL's raw sections")


def patches():
    source = (Path(__file__).parents[1] / "src" / "species" / "patches.rs").read_text()
    pattern = (r"Patch\s*\{\s*rva:\s*(0x[\da-fA-F_]+),\s*"
               r"original:\s*&\[([^]]*)\],\s*replacement:\s*&\[([^]]*)\]")
    result = {}
    for rva, old, new in re.findall(pattern, source):
        rva = int(rva, 16)
        assert rva not in result, f"duplicate patch at {rva:#x}"
        decode = lambda text: bytes(int(x, 16) for x in re.findall(r"0x[\da-fA-F_]+", text))
        result[rva] = decode(old), decode(new)
    assert set(result) == {check.patch for check in CHECKS}, "expected exactly the eight checked patches"
    return result


def execute(check, code, species):
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    for address, size in [(CODE, 0x1000), (ACTOR, 0x4000), (STACK, 0x4000)]:
        uc.mem_map(address, size)
    actor = bytearray(b"\xa5" * 0x1000)
    actor[3] = species
    uc.mem_write(ACTOR, bytes(actor))
    spawn = bytearray(0x40)
    spawn[0], spawn[2], spawn[0x2C] = species, 0x37, 0x6D
    uc.mem_write(SPAWN, bytes(spawn))
    uc.mem_write(STACK, b"\x5a" * 0x4000)
    for register in [reg.UC_X86_REG_EAX, reg.UC_X86_REG_EBX, reg.UC_X86_REG_ECX,
                     reg.UC_X86_REG_EDX, reg.UC_X86_REG_ESI, reg.UC_X86_REG_EDI]:
        uc.reg_write(register, 0x12340000)
    uc.reg_write(reg.UC_X86_REG_ESI, ACTOR)
    uc.reg_write(reg.UC_X86_REG_ESP, SP)
    uc.reg_write(reg.UC_X86_REG_EBP, BP)
    uc.reg_write(reg.UC_X86_REG_EFLAGS, 0x202)
    if check.name == "creation":
        uc.reg_write(reg.UC_X86_REG_ESI, SPAWN)
        uc.reg_write(reg.UC_X86_REG_EBX, ACTOR)
    elif check.name == "update":
        # The preceding test/jnz only reaches this check when EAX is zero.
        uc.reg_write(reg.UC_X86_REG_EAX, 0)
    elif check.name == "model parameters":
        uc.reg_write(reg.UC_X86_REG_ESI, species)
    elif check.name == "sound":
        uc.reg_write(reg.UC_X86_REG_EAX, 0x12340000 | species)
    elif check.name == "additional sound":
        uc.reg_write(reg.UC_X86_REG_ECX, 0x12340000 | species)
    uc.mem_write(CODE, code)
    recovered = []
    recovery = CODE + check.recovery - check.start

    def on_recovery(_uc, _address, _size, _data):
        recovered.append(True)

    uc.hook_add(UC_HOOK_CODE, on_recovery, begin=recovery, end=recovery)
    stop = CODE + len(code)
    uc.emu_start(CODE, stop, count=32)
    assert uc.reg_read(reg.UC_X86_REG_EIP) == stop, f"{check.name}: failed to reach merge"
    assert len(recovered) <= 1
    selected = (uc.reg_read(check.result_register) if check.result_register is not None
                else uc.mem_read(ACTOR + 3, 1)[0])
    if check.result_register is None:
        actor[3] = selected
    elif check.name == "creation":
        actor[0x1B], actor[0xC7C] = spawn[2], spawn[0x2C]
    assert uc.mem_read(ACTOR, len(actor)) == bytes(actor), f"{check.name}: unexpected actor write"
    assert uc.mem_read(SPAWN, len(spawn)) == bytes(spawn), f"{check.name}: spawn modified"
    expected_stack = bytearray(b"\x5a" * 0x4000)
    pushed = check.name == "initialization"
    if pushed:
        struct.pack_into("<I", expected_stack, SP - STACK - 4, 0x12340000)
    if check.name == "sound":
        struct.pack_into("<I", expected_stack, BP - STACK - 0x118, species)
    assert uc.mem_read(STACK, 0x4000) == bytes(expected_stack), f"{check.name}: unexpected stack write"
    assert uc.reg_read(reg.UC_X86_REG_ESP) == SP - (4 if pushed else 0)
    return selected, bool(recovered)


def verify(data):
    spans = sections(data)
    edits = patches()
    executions = 0
    for check in CHECKS:
        old, new = edits[check.patch]
        assert old and len(old) == len(new), f"{check.name}: patch changed instruction length"
        assert native_bytes(data, spans, check.patch, len(old)) == old, f"signature mismatch: {check.patch:#x}"
        changed = [(a, b) for a, b in zip(old, new) if a != b]
        assert changed == [(177, 255)], f"{check.name}: expected only the ID limit to change"
        for instruction_bytes, limit in [(old, 177), (new, 255)]:
            instructions = list(DISASM.disasm(instruction_bytes, check.patch))
            assert len(instructions) == 1 and instructions[0].size == len(instruction_bytes)
            instruction = instructions[0]
            assert instruction.mnemonic in ("cmp", "mov")
            assert instruction.operands[-1].type == CS_OP_IMM and instruction.operands[-1].imm == limit
        original = native_bytes(data, spans, check.start, check.stop - check.start)
        instructions = list(DISASM.disasm(original, check.start))
        assert sum(i.size for i in instructions) == len(original), f"{check.name}: incomplete snippet"
        branches = [i for i in instructions if i.mnemonic.startswith("j")]
        assert len(branches) == 1 and branches[0].mnemonic == "jb", f"{check.name}: expected unsigned comparison"
        assert branches[0].operands[0].imm == check.stop, f"{check.name}: unexpected merge"
        assert instructions[-1].address == check.recovery
        offset = check.patch - check.start
        modified = original[:offset] + new + original[offset + len(old):]
        for species in range(256):
            before = execute(check, original, species)
            after = execute(check, modified, species)
            fallback = 1 if check.result_register is not None else 0
            assert before == (species if species < 177 else fallback, species >= 177), (check.name, species, "original", before)
            assert after == (species if species < 255 else fallback, species == 255), (check.name, species, "patched", after)
            if species < 177 or species == 255:
                assert before == after, (check.name, species, "existing behavior changed")
            executions += 2
        print(f"PASS: {check.name}: all 256 IDs, original and patched branches")
    print(f"PASS: {len(edits)} DLL signatures and equal-length patches; {executions} native snippet executions")
    print("IDs 0..176 retain their selection; 177..254 pass; 255 retains its original fallback.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("client", type=Path, help="unmodified mhfo-hd.dll")
    args = parser.parse_args()
    data = args.client.read_bytes()
    print(f"DLL SHA-256: {hashlib.sha256(data).hexdigest()}")
    verify(data)


if __name__ == "__main__":
    main()
