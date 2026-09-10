/* Generated from Rust by safer-ffi. Do not edit. */
#ifndef MHF_TRANSLATION_H
#define MHF_TRANSLATION_H

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
/** \remark Has the same ABI as `uint8_t` **/
#ifdef DOXYGEN
typedef
#endif
enum TranslationKeyKind {
    /** <No documentation available> */
    TRANSLATION_KEY_KIND_RESOURCE = 1,
    /** <No documentation available> */
    TRANSLATION_KEY_KIND_STAGE = 2,
}
#ifndef DOXYGEN
; typedef uint8_t
#endif
TranslationKeyKind_t;

/** \brief
 *  A borrowed key containing stable resource identifiers, never catalog ordinals.
 *  Fields for the other kind are ignored. C callers must provide valid UTF-8
 *  string slices with non-null pointers, including empty slices; all borrowed
 *  bytes must remain immutable and live for the resolve call.
 */
typedef struct TranslationKey {
    /** <No documentation available> */
    TranslationKeyKind_t kind;

    /** <No documentation available> */
    slice_ref_uint8_t resource_id;

    /** <No documentation available> */
    slice_ref_uint8_t group_id;

    /** <No documentation available> */
    uint32_t record_id;

    /** <No documentation available> */
    uint16_t part;

    /** <No documentation available> */
    uint16_t stage_id;

    /** <No documentation available> */
    uint16_t section;

    /** <No documentation available> */
    uint16_t record;
} TranslationKey_t;

typedef TranslationKey_t TranslationKey;

/** <No documentation available> */
typedef struct Erased Erased_t;

/** <No documentation available> */
typedef struct TranslationApiVTable {
    /** <No documentation available> */
    void (*release_vptr)(Erased_t *);

    /** \brief
     *  Copies UTF-8 bytes without a terminating NUL. `required` counts bytes.
     *  `BUFFER_TOO_SMALL` writes that count and no partial translation; `OK`
     *  writes exactly that many bytes. `NOT_FOUND` requests original source
     *  handling; `OK` with `required = 0` explicitly replaces it with empty text.
     *
     *  C callers must provide a writable, non-null `required` pointer and a
     *  valid mutable slice, whose pointer is non-null even for a size probe.
     *  Outputs must not overlap each other, the key's string bytes, provider
     *  storage, or another active access. Keep all borrows and provider code live
     *  for the call; the generated signature preserves these borrows for Rust.
     */
    int32_t (*resolve)(Erased_t const *, TranslationKey_t, slice_mut_uint8_t, size_t *);
} TranslationApiVTable_t;

/** <No documentation available> */
typedef struct VirtualPtr__Erased_ptr_TranslationApiVTable {
    /** <No documentation available> */
    Erased_t * ptr;

    /** <No documentation available> */
    TranslationApiVTable_t vtable;
} VirtualPtr__Erased_ptr_TranslationApiVTable_t;

typedef VirtualPtr__Erased_ptr_TranslationApiVTable_t TranslationTable;

#define MHF_TRANSLATION_PROVIDER_ID "mhf.translation"
#define MHF_TRANSLATION_INTERFACE_ID "mhf.translation.v1"

#ifdef __cplusplus
}
#endif
#endif /* MHF_TRANSLATION_H */
