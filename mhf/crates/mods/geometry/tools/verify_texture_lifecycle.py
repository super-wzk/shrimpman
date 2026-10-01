"""Replay the native weapon texture-bank overlap and handle-reuse failure.

The supplied DLL is read only and runs in private Unicorn memory. The real
108DF7D0 loader, 108DF700 cleanup, 100113F0 reservation, 1158FFD0 dispatch and
10011960 destruction, 108F9080 resource cleanup and 1000AC00 renderer sweep
execute unchanged. Win32 synchronization, GPU creation, unrelated renderer
objects and COM Release are stubbed. This proves the multiple-texture overlap condition;
it does not establish that a reported single-texture weapon triggers that bug.
The optional owner guard is a Python policy model around the real native
destructor, not execution of the compiled Rust hooks or an in-game test.
"""

import argparse
import json
import struct
from pathlib import Path

from unicorn import UC_HOOK_CODE

from verify_equipment_cache import (
    HEAP, mapped_client, read32, return_stub, run, write32,
)
from verify_native import CODE, reg

TEXTURE_BANK = 0x1ECDA320
TEXTURE_REGISTRY = 0x11AA7D80
REGISTRY_STRIDE = 216
ENTER = CODE + 0x3110
LEAVE = CODE + 0x3120
RELEASE = CODE + 0x3130
THREAD = CODE + 0x3140


def verify_overlap(data, multiple_textures, guarded=False, bulk=False,
                   renderer=False, retire_renderer=True):
    uc = mapped_client(data)
    for location in [ENTER, LEAVE, RELEASE]:
        uc.mem_write(location, b"\xc2\x04\x00")  # stdcall, one argument
    uc.mem_write(THREAD, b"\xb8\x01\x00\x00\x00\xc3")
    write32(uc, 0x115D213C, ENTER)
    write32(uc, 0x115D2138, LEAVE)
    write32(uc, 0x115D2204, THREAD)
    # The native bridge directly invokes callbacks off its worker thread.
    write32(uc, 0x1E866D0C, 0)
    vtable = HEAP + 0x10000
    write32(uc, vtable + 8, RELEASE)
    creations = []
    releases = []
    payloads = {}
    protected = set()
    deferred = []
    renderer_stubs = [0x115ABCC5, 0x10012EE0, 0x1000A1C0]

    def hook(emu, location, _size, _user):
        sp = emu.reg_read(reg.UC_X86_REG_ESP)
        argument = lambda index: read32(emu, sp + 4 + 4 * index)
        if location in renderer_stubs:
            return_stub(emu, 0)
        elif location == 0x10011960 and argument(0) in protected:
            deferred.append(argument(0))
            return_stub(emu, 0)
        elif location == 0x10011AF0:
            handle = argument(2)
            assert read32(emu, TEXTURE_REGISTRY + REGISTRY_STRIDE * handle) == 0xFFFFFFFF
            payload = bytes(emu.mem_read(argument(0), argument(1))).decode("ascii")
            texture = HEAP + 0x11000 + 32 * handle
            write32(emu, texture, vtable)
            write32(emu, texture + 4, handle)
            write32(emu, TEXTURE_REGISTRY + REGISTRY_STRIDE * handle, texture)
            creations.append((handle, payload))
            payloads[handle] = payload
            return_stub(emu, handle)
        elif location == RELEASE:
            releases.append(read32(emu, argument(0) + 4))

    for location in [0x10011AF0, 0x10011960, RELEASE, *renderer_stubs]:
        uc.hook_add(UC_HOOK_CODE, hook, begin=location, end=location)

    def load(base, offset, names):
        resource = HEAP + offset
        payload_start = 4 + 8 * len(names)
        entries = []
        payload = b""
        for name in names:
            content = name.encode("ascii")
            entries.extend([payload_start + len(payload), len(content)])
            payload += content
        header = struct.pack("<" + "I" * (1 + len(entries)), len(names), *entries)
        uc.mem_write(resource, header + payload)
        uc.reg_write(reg.UC_X86_REG_EAX, base)
        run(uc, 0x108DF7D0, [resource])
        assert uc.reg_read(reg.UC_X86_REG_EAX) == len(names)
        return [read32(uc, TEXTURE_BANK + 4 * (base + index)) for index in range(len(names))]

    # Native plaza weapon banks begin at 217 + player slot.
    first = load(220, 0x20000, ["A0", "A1"] if multiple_textures else ["A0"])
    second = load(221, 0x21000, ["B0"])
    second_handle = second[0]
    before = payloads[second_handle]
    if guarded and not bulk:
        protected.update(second)
    # The first resource retains its original base/count, while the adjacent
    # bank entry may now hold a texture belonging to the second resource.
    if renderer:
        # Renderer teardown bypasses resource cleanup and scans all 4095 native
        # registry slots, so it also sees the orphaned first owner's handle.
        # Retiring the ownership metadata before that sweep must unblock all
        # completed textures. Keeping metadata reproduces the teardown leak.
        if guarded and retire_renderer:
            protected.clear()
        run(uc, 0x1000AC00)
    elif bulk:
        # Bulk cleanup inlines resource destruction instead of calling 108F8EB0.
        # The minimal live resources need only state, texture count and base;
        # their object/material/animation allocations are all empty.
        resources = HEAP + 0x100000
        write32(uc, 0x1ED528D0, resources)
        for index, base, count in [(0, 220, len(first)), (1, 221, len(second))]:
            uc.mem_write(resources + 128 * index, struct.pack("<H", 1))
            uc.mem_write(resources + 128 * index + 8, struct.pack("<HH", count, base))
        run(uc, 0x108F9080)
    else:
        run(uc, 0x108DF700, [220, len(first), 0])
    mapping_after_cleanup = read32(uc, TEXTURE_BANK + 4 * 221)
    if guarded and not renderer:
        # Supplement native bank cleanup with the first owner's captured
        # identities. Keep the ownership check inside the dispatch callback.
        for handle in [*first, *second] if bulk else first:
            if read32(uc, TEXTURE_REGISTRY + REGISTRY_STRIDE * handle) != 0:
                uc.reg_write(reg.UC_X86_REG_EDI, 0x10011960)
                uc.reg_write(reg.UC_X86_REG_ESI, handle)
                run(uc, 0x1158FFD0)
    destroyed = read32(uc, TEXTURE_REGISTRY + REGISTRY_STRIDE * second_handle) == 0
    remaining = [handle for handle in [*first, *second]
                 if read32(uc, TEXTURE_REGISTRY + REGISTRY_STRIDE * handle) != 0]
    grass = load(2763, 0x22000, ["GRASS"])
    clothing = load(329, 0x23000, ["CLOTH"])
    after = payloads[second_handle]

    if renderer:
        assert multiple_textures and mapping_after_cleanup == second_handle
        if guarded and not retire_renderer:
            assert releases == [1, 2] and deferred == [3] and not destroyed
            assert remaining == [3]
        else:
            assert releases == [1, 2, 3] and destroyed and not remaining
    elif bulk:
        assert multiple_textures and destroyed and mapping_after_cleanup == 0
        if guarded:
            assert releases == [1, 3, 2] and not remaining
        else:
            assert releases == [1, 3] and remaining == [2]
    elif guarded:
        assert multiple_textures and first == [1, 2] and second == [3]
        assert releases == [1, 2] and deferred == [3] and not destroyed
        assert mapping_after_cleanup == second_handle, "deferred release cleared the live owner's bank"
        assert grass == [1] and clothing == [2] and after == "B0"
    elif multiple_textures:
        assert first == [1, 2] and second == [3]
        assert before == "B0" and releases == [1, 3] and destroyed
        assert read32(uc, TEXTURE_REGISTRY + REGISTRY_STRIDE * 2) != 0, "overwritten A1 handle was not retained"
        assert grass == [1] and clothing == [3] and after == "CLOTH"
    else:
        assert first == [1] and second == [2]
        assert before == "B0" and releases == [1] and not destroyed
        assert grass == [1] and clothing == [3] and after == "B0"
    return {
        "multiple_texture_weapon": multiple_textures,
        "modeled_owner_guard": guarded,
        "native_bulk_cleanup": bulk,
        "native_renderer_cleanup": renderer,
        "renderer_owners_retired": renderer and retire_renderer,
        "first_handles": first,
        "second_handles": second,
        "second_payload_before_release": before,
        "native_releases": releases,
        "deferred_releases": deferred,
        "second_bank_after_first_cleanup": mapping_after_cleanup,
        "second_destroyed_by_first_cleanup": destroyed,
        "handles_remaining_after_cleanup": remaining,
        "grass_handles": grass,
        "clothing_handles": clothing,
        "second_numeric_handle_payload_after_other_loads": after,
        "native_creations": creations,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("client", type=Path)
    args = parser.parse_args()
    data = args.client.read_bytes()
    cases = [
        verify_overlap(data, False),
        verify_overlap(data, True),
        verify_overlap(data, True, guarded=True),
        verify_overlap(data, True, bulk=True),
        verify_overlap(data, True, guarded=True, bulk=True),
        verify_overlap(data, True, guarded=True, renderer=True, retire_renderer=False),
        verify_overlap(data, True, guarded=True, renderer=True),
    ]
    print(json.dumps(cases, indent=2))
    print("PASS: single-texture adjacent banks retain independent native texture handles")
    print("PASS: multiple-texture overlap reproduces wrong-owner destruction, an orphaned handle and clothing-handle reuse")
    print("PASS: modeled owner guard preserves the live bank and snapshot cleanup frees the orphaned texture")
    print("PASS: real bulk resource cleanup bypasses the per-resource entrypoint; retired owner snapshots free its orphaned handle")
    print("PASS: real renderer cleanup sweeps orphaned handles; retiring owners before the sweep prevents the guard from leaking live weapon textures")
    print("NOTE: no in-game rendering or compiled Rust ownership hook is verified by this fixture")


if __name__ == "__main__":
    main()
