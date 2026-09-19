"""Reproduce the truncated AI hang using original ZZ HD machine code in Unicorn.

The game DLL is read-only. No running game is attached or modified.
Dependencies: unicorn, capstone (the shared PE helper imports capstone).
"""

import argparse
import hashlib
import struct
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
from unicorn import x86_const as reg
from verify_native import native_bytes, sections


def check(data):
    assert hashlib.sha256(data).hexdigest() == "95c580195f4080d2e9582c8c9df36abeb280476e088b6366583c5f138da8f301"
    spans = sections(data)
    code = native_bytes(data, spans, 0x860000, 0x10000)
    original = native_bytes(data, spans, 0x179CEA4, 56)
    assert original[39:43] == bytes([0xff, 0, 0x39, 2])
    script, actor, stack, returned = 0x40000000, 0x50000000, 0x60000000, 0x70000000
    for truncated in [True, False]:
        uc = Uc(UC_ARCH_X86, UC_MODE_32)
        uc.mem_map(0x10860000, 0x10000)
        uc.mem_write(0x10860000, code)
        for address in [script, actor, stack, returned]:
            uc.mem_map(address, 0x4000)
        # Zero padding matches the binder's guard. The same native handler is
        # used for both cases, with actor+2914 == 0 selecting the skip branch.
        uc.mem_write(script, original[:41] if truncated else original)
        uc.mem_write(stack + 0x2000, struct.pack("<I", returned))
        uc.reg_write(reg.UC_X86_REG_ESP, stack + 0x2000)
        uc.reg_write(reg.UC_X86_REG_ESI, actor)
        uc.reg_write(reg.UC_X86_REG_EAX, script + 1)
        uc.emu_start(0x10863750, returned, count=10000)
        pc = uc.reg_read(reg.UC_X86_REG_EIP)
        if truncated:
            assert pc != returned, "truncated script unexpectedly returned"
            assert uc.mem_read(actor + 3182, 1) == b"\x04"
            print(f"REPRODUCED: old 41-byte export exhausts 10000 instructions in native scan at {pc:#x}")
        else:
            assert pc == returned, f"complete script stalled at {pc:#x}"
            assert uc.reg_read(reg.UC_X86_REG_EAX) == script + 43
            print("PASS: full 56-byte script returns after 39/02 to the fallback branch")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("client", type=Path)
    check(parser.parse_args().client.read_bytes())
