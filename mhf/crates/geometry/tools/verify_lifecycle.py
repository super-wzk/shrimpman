"""Execute the ZZ HD task-model teardown against extended model allocations.

Only scheduling, COM Release, critical sections and CRT free are simulated.
The task cleanup, buffer-release callback and allocation unlinker execute from
the supplied DLL in private Unicorn memory. This verifies their memory contract,
not D3D9 driver behavior or the creation and rendering of a real game asset.
"""

import argparse
import struct
import sys
from collections import Counter
from pathlib import Path

from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE

sys.dont_write_bytecode = True
from verify_native import CODE, DATA, STOP, cpu, reg, sections, u32

BASE = 0x10000000
TEARDOWN = 0x108F8DD0
RELEASE_BUFFERS = 0x100081E0
UNLINK = 0x10001000
SCHEDULE = 0x1158FFD0
FREE = 0x115AB644
REGISTRY = 0x11AA3DD8
ACTIVE_COUNT = 0x1E73D384
HEAD = 0x1E73D370
CRITICAL_SECTION = 0x1E73ACF8
ENTER_IAT = 0x115D213C
LEAVE_IAT = 0x115D2138

ENTER = CODE + 0x3000
LEAVE = CODE + 0x3010
RELEASE = CODE + 0x3020
DISPATCH = CODE + 0x3030
HEAP = 0x30000000
HEAP_SIZE = 0x1000000
RECORDS = DATA + 0x1000
VTABLE = DATA + 0x3000
CANARY = b"\xa5" * 32


def write32(uc, address, value):
    uc.mem_write(address, struct.pack("<I", value))


def read32(uc, address):
    return u32(uc.mem_read(address, 4))


def return_from_stub(uc, pop=0, result=0):
    sp = uc.reg_read(reg.UC_X86_REG_ESP)
    uc.reg_write(reg.UC_X86_REG_EIP, read32(uc, sp))
    uc.reg_write(reg.UC_X86_REG_ESP, sp + 4 + pop)
    uc.reg_write(reg.UC_X86_REG_EAX, result)


def verify_case(data, vertex_count, scenario, skinned):
    uc = cpu()
    pe = u32(data, 0x3C)
    image_size = u32(data, pe + 24 + 56)
    uc.mem_map(BASE, (image_size + 0xFFF) & ~0xFFF)
    for rva, size, offset in sections(data):
        if size:
            uc.mem_write(BASE + rva, data[offset:offset + size])
    uc.mem_map(HEAP, HEAP_SIZE)

    # stdcall imports and COM methods are callbacks below. The scheduler pushes
    # its ESI argument and calls EDI with the actual cdecl callback convention.
    for address in [ENTER, LEAVE, RELEASE]:
        uc.mem_write(address, b"\xc3")
    uc.mem_write(DISPATCH, b"\x56\xff\xd7\x83\xc4\x04\xc3")
    write32(uc, ENTER_IAT, ENTER)
    write32(uc, LEAVE_IAT, LEAVE)
    write32(uc, VTABLE + 8, RELEASE)

    models = []
    cursor = HEAP + len(CANARY)
    counts = [vertex_count] if scenario == "single" else [3, vertex_count, 70001]
    for index, count in enumerate(counts):
        handle = index + 1
        allocation = cursor
        model = allocation + 16
        cpu_bytes = count * 16 if skinned else 0
        body_size = 96 + 20 + 4 + cpu_bytes
        allocation_size = 16 + body_size
        cursor = (allocation + allocation_size + len(CANARY) + 63) & ~15
        assert cursor < HEAP + HEAP_SIZE
        uc.mem_write(allocation - len(CANARY), CANARY)
        uc.mem_write(allocation, b"\0" * allocation_size)
        uc.mem_write(allocation + allocation_size, CANARY)

        vb = DATA + 0x4000 + index * 32
        ib = vb + 16
        write32(uc, vb, VTABLE)
        write32(uc, ib, VTABLE)
        header = [0] * 24
        header[0] = 1
        header[3] = body_size
        header[4] = 5
        header[5] = 116
        header[6] = 1
        header[7] = vb
        header[8] = ib
        header[9] = 2  # D3DFVF_XYZ
        header[10] = count
        header[11] = 12
        if skinned:
            header[15:20] = [6, count, 16, 0, 120]  # D3DFVF_XYZB1
        header[20] = 120
        uc.mem_write(model, struct.pack("<24I", *header))
        # A DWORD descriptor exercises a length above the old WORD boundary.
        write32(uc, model + 116, count)
        if cpu_bytes:
            uc.mem_write(model + 120, b"\x5a" * cpu_bytes)
        write32(uc, REGISTRY + handle * 4, model)
        models.append({
            "handle": handle, "allocation": allocation, "model": model,
            "size": allocation_size, "buffers": (vb, ib),
            "body": bytes(uc.mem_read(model, body_size)),
        })

    for index, model in enumerate(models):
        next_node = models[index + 1]["allocation"] if index + 1 < len(models) else 0
        previous = models[index - 1]["allocation"] if index else 0
        write32(uc, model["allocation"], next_node)
        write32(uc, model["allocation"] + 4, previous)
    write32(uc, HEAD, models[0]["allocation"])
    write32(uc, ACTIVE_COUNT, len(models))

    handles = [model["handle"] for model in models]
    if scenario == "middle":
        records = [handles[1]]
    elif scenario == "duplicates":
        records = [0, handles[1], handles[1], 0xFFFFFFFF, handles[0], handles[2], handles[0]]
    elif scenario == "reverse":
        records = handles[::-1]
    else:
        records = handles
    uc.mem_write(RECORDS - len(CANARY), CANARY)
    for index, handle in enumerate(records):
        uc.mem_write(RECORDS + index * 140, struct.pack("<I", handle) + b"\xcc" * 136)
    uc.mem_write(RECORDS + len(records) * 140, CANARY)
    original_records = bytes(uc.mem_read(RECORDS, len(records) * 140))

    expected_handles = set(records) - {0, 0xFFFFFFFF}
    expected_models = [model for model in models if model["handle"] in expected_handles]
    live_models = [model for model in models if model["handle"] not in expected_handles]
    allocations = {model["allocation"]: model for model in models}
    com_objects = {buffer for model in models for buffer in model["buffers"]}
    releases = Counter()
    freed = []
    scheduled = []
    native_calls = Counter()
    lock_depth = 0
    lock_calls = Counter()

    def on_code(emu, address, _size, _user):
        nonlocal lock_depth
        sp = emu.reg_read(reg.UC_X86_REG_ESP)
        if address in (TEARDOWN, RELEASE_BUFFERS, UNLINK):
            native_calls[address] += 1
        elif address == SCHEDULE:
            callback = emu.reg_read(reg.UC_X86_REG_EDI)
            argument = emu.reg_read(reg.UC_X86_REG_ESI)
            assert callback == RELEASE_BUFFERS, hex(callback)
            assert argument in {model["model"] for model in expected_models}, hex(argument)
            scheduled.append(argument)
            emu.reg_write(reg.UC_X86_REG_EIP, DISPATCH)
        elif address == RELEASE:
            buffer = read32(emu, sp + 4)
            assert buffer in com_objects, f"invalid COM pointer {buffer:#x}"
            releases[buffer] += 1
            assert releases[buffer] == 1, f"double COM Release {buffer:#x}"
            return_from_stub(emu, pop=4)
        elif address in (ENTER, LEAVE):
            assert read32(emu, sp + 4) == CRITICAL_SECTION
            lock_calls[address] += 1
            lock_depth += 1 if address == ENTER else -1
            assert lock_depth == (1 if address == ENTER else 0)
            return_from_stub(emu, pop=4)
        elif address == FREE:
            allocation = read32(emu, sp + 4)
            assert allocation in allocations, f"free used wrong base {allocation:#x}"
            assert allocation not in freed, f"double free {allocation:#x}"
            assert lock_depth == 0, "free called while holding the list lock"
            model = allocations[allocation]
            assert all(releases[buffer] == 1 for buffer in model["buffers"])
            assert read32(emu, model["model"] + 0x1C) == 0
            assert read32(emu, model["model"] + 0x20) == 0
            freed.append(allocation)
            return_from_stub(emu)

    def on_heap_access(_emu, _access, address, size, _value, _user):
        owner = next((model for model in models if
                      model["allocation"] <= address and
                      address + size <= model["allocation"] + model["size"]), None)
        assert owner is not None, f"out-of-allocation access {address:#x}+{size}"
        assert owner["allocation"] not in freed, f"use after free {address:#x}"

    for address in [TEARDOWN, RELEASE_BUFFERS, UNLINK, SCHEDULE, RELEASE, ENTER, LEAVE, FREE]:
        uc.hook_add(UC_HOOK_CODE, on_code, begin=address, end=address)
    uc.hook_add(UC_HOOK_MEM_READ | UC_HOOK_MEM_WRITE, on_heap_access,
                begin=HEAP, end=HEAP + HEAP_SIZE - 1)
    sp = uc.reg_read(reg.UC_X86_REG_ESP)
    write32(uc, sp, STOP)
    uc.reg_write(reg.UC_X86_REG_EAX, len(records))
    uc.reg_write(reg.UC_X86_REG_ECX, RECORDS)
    preserved = {register: uc.reg_read(register) for register in
                 [reg.UC_X86_REG_EBX, reg.UC_X86_REG_ESI, reg.UC_X86_REG_EDI, reg.UC_X86_REG_EBP]}
    uc.emu_start(TEARDOWN, STOP, count=10000)
    assert uc.reg_read(reg.UC_X86_REG_EIP) == STOP, "teardown did not return"
    assert uc.reg_read(reg.UC_X86_REG_ESP) == sp + 4, "teardown changed caller stack"
    for register, value in preserved.items():
        assert uc.reg_read(register) == value, "teardown changed callee-saved register"

    expected_count = len(expected_models)
    assert native_calls == {TEARDOWN: 1, RELEASE_BUFFERS: expected_count, UNLINK: expected_count}
    assert Counter(freed) == Counter(model["allocation"] for model in expected_models)
    assert Counter(scheduled) == Counter(model["model"] for model in expected_models)
    assert releases == Counter(buffer for model in expected_models for buffer in model["buffers"])
    assert lock_depth == 0
    assert lock_calls == {ENTER: expected_count, LEAVE: expected_count}
    assert read32(uc, ACTIVE_COUNT) == len(live_models)
    assert read32(uc, HEAD) == (live_models[0]["allocation"] if live_models else 0)
    for index, model in enumerate(live_models):
        previous = live_models[index - 1]["allocation"] if index else 0
        following = live_models[index + 1]["allocation"] if index + 1 < len(live_models) else 0
        assert read32(uc, model["allocation"]) == following
        assert read32(uc, model["allocation"] + 4) == previous
    for model in models:
        removed = model["handle"] in expected_handles
        assert read32(uc, REGISTRY + model["handle"] * 4) == (0 if removed else model["model"])
        expected_body = bytearray(model["body"])
        if removed:
            expected_body[0x1C:0x24] = b"\0" * 8
        assert uc.mem_read(model["model"], len(expected_body)) == expected_body
        assert uc.mem_read(model["allocation"] - len(CANARY), len(CANARY)) == CANARY
        assert uc.mem_read(model["allocation"] + model["size"], len(CANARY)) == CANARY
        assert uc.mem_read(model["allocation"] + 8, 8) == b"\0" * 8
    assert uc.mem_read(RECORDS, len(original_records)) == original_records
    assert uc.mem_read(RECORDS - len(CANARY), len(CANARY)) == CANARY
    assert uc.mem_read(RECORDS + len(original_records), len(CANARY)) == CANARY


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("client", type=Path)
    args = parser.parse_args()
    data = args.client.read_bytes()
    count = 0
    for vertices in [3, 65535, 65536, 70001]:
        for skinned in [False, True]:
            for scenario in ["single", "middle", "all", "reverse", "duplicates"]:
                try:
                    verify_case(data, vertices, scenario, skinned)
                except Exception as error:
                    raise AssertionError(
                        f"vertices={vertices}, cpu_skinning={skinned}, scenario={scenario}: {error}"
                    ) from error
                count += 1
    print(f"PASS: {count} real native task-model teardown cases at 3/65535/65536/70001 vertices")
    print("PASS: single/multiple nodes, middle/all/reverse removal, duplicate and empty handles")
    print("PASS: one COM Release per buffer, native free bases, registry/count/list, canaries and no use after free")


if __name__ == "__main__":
    main()
