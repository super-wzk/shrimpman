"""Compare the complete native GPU model renderer with DWORD descriptors.

The supplied DLL is read only; execution uses private Unicorn memory. The
complete 10018D30 renderer and 10017D10 material/texture selection run from the
DLL. Fake D3D9 methods record SetTexture and DrawIndexedPrimitive calls.

Stubs replace matrix transposition, visibility/culling (1000CCF0, 10006F00),
shader setup (10018930, 10018A70, 10018B50), and the security-cookie check.
The variant argument to 10018B50 is recorded before its stub returns. Material
fixtures disable extra lighting/effect paths and use distinct texture handles.
This covers descriptor traversal and texture binding for the GPU renderer;
it does not verify CPU renderers, shader contents, COM lifetime, or the game.
Development-only dependencies: unicorn and capstone.
"""

import argparse
import struct
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_INVALID

from verify_equipment_cache import read32, write32
from verify_native import patches, reg, sections

BASE = 0x10000000
MODEL = 0x50001000
DEVICE = 0x50000000
VTABLE = 0x50000100
CONTEXT = 0x50000800
STOP = 0x4000F000
STACK = 0x60008000
TRANSPOSE = 0x4000E000

# Vtable offsets and stdcall argument counts, including the COM this pointer.
D3D_ARGUMENTS = {
    400: 5,  # SetStreamSource
    416: 2,  # SetIndices
    376: 4,  # SetVertexShaderConstantF
    436: 4,  # SetPixelShaderConstantF
    180: 3,  # GetTransform
    328: 7,  # DrawIndexedPrimitive
    260: 3,  # SetTexture
    276: 4,  # SetTextureStageState
}


def render(data, descriptors, flags, bones, wide):
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    regions = [
        (BASE, 0x30000),
        (0x11AA0000, 0xF0000),
        (0x11990000, 0x40000),
        (0x1E73D000, 0x1000),
        (0x1E811000, 0x1000),
        (0x1E87E000, 0x2000),
        (0x115D2000, 0x1000),
        (0x115AB000, 0x1000),
        (0x40000000, 0x10000),
        (0x50000000, 0x20000),
        (0x60000000, 0x10000),
    ]
    for address, size in regions:
        uc.mem_map(address, size)
    for rva, size, offset in sections(data):
        if rva < 0x30000:
            size = min(size, 0x30000 - rva)
            uc.mem_write(BASE + rva, data[offset:offset + size])
    for rva, original, replacement in patches():
        assert bytes(uc.mem_read(BASE + rva, len(original))) == original, hex(rva)
        if wide:
            uc.mem_write(BASE + rva, replacement)

    write32(uc, DEVICE, VTABLE)
    write32(uc, 0x1E811A3C, DEVICE)
    write32(uc, 0x1E73D3C0, CONTEXT)
    write32(uc, 0x1199CB64, 0x3F800000)
    write32(uc, 0x115D240C, TRANSPOSE)
    d3d_stubs = {0x40000000 + index * 4: index for index in D3D_ARGUMENTS}
    for address, index in d3d_stubs.items():
        write32(uc, VTABLE + index, address)
        uc.mem_write(address, b"\x31\xc0\xc2" + struct.pack("<H", D3D_ARGUMENTS[index] * 4))
    uc.mem_write(TRANSPOSE, b"\xc2\x08\x00")
    uc.mem_write(0x115AB635, b"\xc3")
    for function in [0x10018930, 0x1000CCF0, 0x10018A70, 0x10018B50, 0x10006F00]:
        uc.mem_write(function, b"\xb8\x01\x00\x00\x00\xc3")

    fields = {
        0: flags,
        4: 2,  # Use the material records stored inside this model.
        16: 5,  # D3DPT_TRIANGLESTRIP.
        20: 0x100,
        24: len(descriptors),
        28: 0x5000,
        32: 0x6000,
        40: sum(cells[0] for cells in descriptors),
        44: 24,
        60: bones,
        80: 0x1000,
    }
    for offset, value in fields.items():
        write32(uc, MODEL + offset, value)
    cells = [value for descriptor in descriptors for value in descriptor]
    width = "I" if wide else "H"
    encoded = struct.pack("<" + width * len(cells), *cells)
    uc.mem_write(MODEL + 0x100, encoded)
    for material in range(40):
        address = MODEL + 0x1000 + 140 * material
        uc.mem_write(address, bytes(140))
        uc.mem_write(address + 68, struct.pack("<H", material + 1))
        for offset in [76, 80, 84]:
            write32(uc, address + offset, 0xFFFFFFFF)
        write32(uc, 0x11AA7D80 + 216 * (material + 1), 0x70000000 + material * 0x100)

    trace = []

    def record(emu, address, _size, _user):
        if address == 0x10017D10:
            trace.append(("material", emu.reg_read(reg.UC_X86_REG_EDX) & 0xFFFF))
        if address == 0x10018B50:
            trace.append(("variant", emu.reg_read(reg.UC_X86_REG_EAX) & 0xFFFF))
        # Only record the COM entry, not the following RET instruction.
        if address in d3d_stubs:
            index = d3d_stubs[address]
            sp = emu.reg_read(reg.UC_X86_REG_ESP)
            arguments = [read32(emu, sp + 4 + i * 4) for i in range(D3D_ARGUMENTS[index])]
            if index == 260:
                trace.append(("texture", *arguments[1:]))
            if index == 328:
                trace.append(("draw", *arguments[1:]))
        if address == TRANSPOSE:
            sp = emu.reg_read(reg.UC_X86_REG_ESP)
            destination, source = read32(emu, sp + 4), read32(emu, sp + 8)
            emu.mem_write(destination, bytes(emu.mem_read(source, 64)))

    def unmapped(emu, _access, address, size, _value, _user):
        raise AssertionError(("unmapped", hex(emu.reg_read(reg.UC_X86_REG_EIP)),
                              hex(address), size, trace))

    uc.hook_add(UC_HOOK_CODE, record)
    uc.hook_add(UC_HOOK_MEM_INVALID, unmapped)
    write32(uc, STACK, STOP)
    for index in range(4):
        write32(uc, STACK + 4 + index * 4, 0)
    uc.reg_write(reg.UC_X86_REG_ESP, STACK)
    uc.reg_write(reg.UC_X86_REG_EDI, MODEL)
    uc.reg_write(reg.UC_X86_REG_EFLAGS, 0x202)
    uc.emu_start(0x10018D30, STOP, count=30_000)
    assert uc.reg_read(reg.UC_X86_REG_EIP) == STOP
    assert bytes(uc.mem_read(MODEL + 0x100, len(encoded))) == encoded
    return trace


def verify_renderer(data):
    count = 0
    for flags in [0, 0x100000, 0x200000, 0x300000]:
        for bones in [0, 1]:
            for lengths in [[3, 7, 5], [32, 4, 128], [65535, 256, 1024]]:
                descriptors = []
                for index, length in enumerate(lengths):
                    cells = [length]
                    if flags & 0x100000:
                        cells.append([1, 17, 3][index])
                    if flags & 0x200000 or flags == 0 and bones:
                        cells.append([0, 17, 1][index])
                    if flags == 0x100000 and bones:
                        cells.append(index)
                    descriptors.append(cells)
                expected = render(data, descriptors, flags, bones, wide=False)
                actual = render(data, descriptors, flags, bones, wide=True)
                assert actual == expected, (hex(flags), bones, descriptors, expected, actual)
                assert sum(event[0] == "draw" for event in actual) == 3
                count += 1
    print(f"PASS: complete 10018D30 WORD/DWORD traces match in {count} cases "
          "including native 10017D10 texture binding")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("client", type=Path)
    args = parser.parse_args()
    verify_renderer(args.client.read_bytes())


if __name__ == "__main__":
    main()
