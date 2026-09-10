/* Generated from Rust by safer-ffi. Do not edit. */
#ifndef EXAMPLE_COUNTER_H
#define EXAMPLE_COUNTER_H

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
typedef struct CounterSnapshot {
    /** <No documentation available> */
    uint32_t count;
} CounterSnapshot_t;

typedef CounterSnapshot_t CounterSnapshot;

/** <No documentation available> */
typedef struct Erased Erased_t;

/** <No documentation available> */
typedef struct CounterApiVTable {
    /** <No documentation available> */
    void (*release_vptr)(Erased_t *);

    /** <No documentation available> */
    CounterSnapshot_t (*snapshot)(Erased_t const *);

    /** <No documentation available> */
    int32_t (*add)(Erased_t const *, uint32_t);
} CounterApiVTable_t;

/** <No documentation available> */
typedef struct VirtualPtr__Erased_ptr_CounterApiVTable {
    /** <No documentation available> */
    Erased_t * ptr;

    /** <No documentation available> */
    CounterApiVTable_t vtable;
} VirtualPtr__Erased_ptr_CounterApiVTable_t;

typedef VirtualPtr__Erased_ptr_CounterApiVTable_t CounterTable;

#define EXAMPLE_COUNTER_PROVIDER "example.counter"
#define EXAMPLE_COUNTER_INTERFACE "example.counter.v1"

#ifdef __cplusplus
}
#endif
#endif /* EXAMPLE_COUNTER_H */
