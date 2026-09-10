/* Generated from Rust by safer-ffi. Do not edit. */
#ifndef EXAMPLE_HOOK_PROBE_H
#define EXAMPLE_HOOK_PROBE_H

/* Host interface lookups return borrowed pointers. Do not free them or
 * consume/copy a generated virtual object as an owner; never call its
 * vtable.release_vptr. Calls and borrowed results must end before provider
 * unloading. Frame callbacks may further limit the borrow. Implementations
 * and callbacks must not unwind across the C ABI. */

#include "mhf_mod.h"

#ifdef __cplusplus
extern "C" {
#endif

/** <No documentation available> */
typedef struct HookSnapshotV1 {
    /** <No documentation available> */
    uint32_t entered;

    /** <No documentation available> */
    uint32_t completed;

    /** <No documentation available> */
    uint32_t active;

    /** <No documentation available> */
    uint32_t drains;
} HookSnapshotV1_t;

typedef HookSnapshotV1_t HookSnapshotV1;

/** <No documentation available> */
typedef struct HookProbeV1 {
    /** <No documentation available> */
    void * context;

    /** <No documentation available> */
    uint32_t (*invoke)(uint32_t);

    /** <No documentation available> */
    int32_t (*snapshot)(void *, HookSnapshotV1_t *);
} HookProbeV1_t;

typedef HookProbeV1_t HookProbeV1;

#define EXAMPLE_HOOK_PROVIDER "example.hook"
#define EXAMPLE_HOOK_INTERFACE "example.hook.v1"

#ifdef __cplusplus
}
#endif
#endif /* EXAMPLE_HOOK_PROBE_H */
