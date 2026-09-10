/* Generated from Rust by safer-ffi. Do not edit. */
#ifndef MHF_DATA_H
#define MHF_DATA_H

/* Host interface lookups return borrowed pointers. Do not free them or
 * consume/copy a generated virtual object as an owner; never call its
 * vtable.release_vptr. Calls and borrowed results must end before provider
 * unloading. Frame callbacks may further limit the borrow. Implementations
 * and callbacks must not unwind across the C ABI. */

#include "mhf_mod.h"

#ifdef __cplusplus
extern "C" {
#endif

/** \brief
 *  Read-only package resources. The table and context remain valid through the
 *  consumer's destroy. Output buffers are supplied and owned by the caller.
 */
typedef struct DataV1 {
    /** <No documentation available> */
    void * context;

    /** <No documentation available> */
    int32_t (*resource_root)(void *, uint8_t *, uint32_t, uint32_t *);

    /** <No documentation available> */
    int32_t (*read_file)(void *, Str_t, uint8_t *, uint32_t, uint32_t *);
} DataV1_t;

typedef DataV1_t MhfDataV1;

#define MHF_DATA_INTERFACE_ID "mhf.data.v1"

#ifdef __cplusplus
}
#endif
#endif /* MHF_DATA_H */
