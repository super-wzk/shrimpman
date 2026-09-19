/* Generated from Rust by safer-ffi. Do not edit. */
#ifndef MHF_GAME_H
#define MHF_GAME_H

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
typedef struct Erased Erased_t;

/** <No documentation available> */
typedef struct ConfigApiVTable {
    /** <No documentation available> */
    void (*release_vptr)(Erased_t *);

    /** <No documentation available> */
    int32_t (*register_section)(Erased_t const *, slice_ref_uint8_t, slice_ref_uint8_t);

    /** \brief
     *  Copies TOML without a NUL terminator. BUFFER_TOO_SMALL sets required
     *  and writes no partial output. Retry if a concurrent write grows it.
     */
    int32_t (*read)(Erased_t const *, slice_ref_uint8_t, slice_mut_uint8_t, size_t *);

    /** \brief
     *  Merge a patch into the latest file, preserving untouched fields.
     *  Failed validation leaves the file intact.
     */
    int32_t (*write)(Erased_t const *, slice_ref_uint8_t, slice_ref_uint8_t);

    /** \brief
     *  Last failure diagnostic; another concurrent failure may replace it.
     */
    int32_t (*last_error)(Erased_t const *, slice_mut_uint8_t, size_t *);
} ConfigApiVTable_t;

/** <No documentation available> */
typedef struct VirtualPtr__Erased_ptr_ConfigApiVTable {
    /** <No documentation available> */
    Erased_t * ptr;

    /** <No documentation available> */
    ConfigApiVTable_t vtable;
} VirtualPtr__Erased_ptr_ConfigApiVTable_t;

typedef VirtualPtr__Erased_ptr_ConfigApiVTable_t ConfigTable;

#define MHF_CONFIG_PROVIDER "mhf.config"
#define MHF_CONFIG_INTERFACE "mhf.config.v1"
/** <No documentation available> */
typedef struct FontApiVTable {
    /** <No documentation available> */
    void (*release_vptr)(Erased_t *);

    /** \brief
     *  The returned bytes remain valid for the object borrow. C consumers must
     *  not write or free them, or retain them beyond the provider's lifetime.
     */
    slice_ref_uint8_t (*family)(Erased_t const *);
} FontApiVTable_t;

/** <No documentation available> */
typedef struct VirtualPtr__Erased_ptr_FontApiVTable {
    /** <No documentation available> */
    Erased_t * ptr;

    /** <No documentation available> */
    FontApiVTable_t vtable;
} VirtualPtr__Erased_ptr_FontApiVTable_t;

typedef VirtualPtr__Erased_ptr_FontApiVTable_t FontTable;

#define MHF_FONT_PROVIDER "mhf.base"
#define MHF_FONT_INTERFACE "mhf.font.v1"

#include <stdbool.h>

/** <No documentation available> */
typedef struct QuestSnapshot {
    /** <No documentation available> */
    uint16_t quest_id;

    /** \brief
     *  The temporary hunter has initialized. Restart does not clear this;
     *  it does not imply the current task, map or actor is ready.
     */
    bool hunter_initialized;

    /** <No documentation available> */
    size_t quest_size;
} QuestSnapshot_t;

typedef QuestSnapshot_t QuestSnapshot;

typedef struct {
    float idx[3];
} float_3_array_t;

/** \brief
 *  Replacement resource species and variant, spawn record and hunter start area. This
 *  prepares quest data; it does not create or control a running monster.
 */
typedef struct QuestMonsterSpawn {
    /** <No documentation available> */
    uint8_t species;

    /** \brief
     *  Native species variant in 0..=16: 0 = normal, 1 = HC, 16 = Zenith.
     *  Other values depend on the species. The caller verifies species support.
     */
    uint8_t variant;

    /** <No documentation available> */
    uint16_t area;

    /** <No documentation available> */
    float_3_array_t position;

    /** <No documentation available> */
    uint16_t yaw;
} QuestMonsterSpawn_t;

typedef QuestMonsterSpawn_t QuestMonsterSpawn;

typedef uint32_t QuestSpawnOffset;

/** <No documentation available> */
typedef struct QuestApiVTable {
    /** <No documentation available> */
    void (*release_vptr)(Erased_t *);

    /** <No documentation available> */
    QuestSnapshot_t (*snapshot)(Erased_t const *);
} QuestApiVTable_t;

/** <No documentation available> */
typedef struct VirtualPtr__Erased_ptr_QuestApiVTable {
    /** <No documentation available> */
    Erased_t * ptr;

    /** <No documentation available> */
    QuestApiVTable_t vtable;
} VirtualPtr__Erased_ptr_QuestApiVTable_t;

typedef VirtualPtr__Erased_ptr_QuestApiVTable_t QuestTable;

/** <No documentation available> */
typedef struct QuestControlApiVTable {
    /** <No documentation available> */
    void (*release_vptr)(Erased_t *);

    /** <No documentation available> */
    QuestSnapshot_t (*snapshot)(Erased_t const *);

    /** \brief
     *  Checks that the provider is installed into the intended loaded game.
     *
     *  # Safety
     *  The module must be live. Call during installation or on the game thread
     *  while this provider's offline hooks are active. This does not retain the
     *  DLL or extend any native-memory lifetime.
     */
    int32_t (*validate_module)(Erased_t const *, void *);

    /** \brief
     *  # Safety
     *  Call on the game thread with offline hooks active, after all consumers
     *  have released native actor references into the old quest. No native
     *  reader may concurrently use the state being restarted.
     */
    int32_t (*restart)(Erased_t const *);

    /** \brief
     *  # Safety
     *  Call on the game thread with offline hooks active, after releasing all
     *  native references to the old override and before restarting the quest
     *  loader. No native reader may use the old override; old offsets expire.
     */
    int32_t (*reset_quest)(Erased_t const *);

    /** \brief
     *  Prepares replacement records; success initializes `out_offset`, while
     *  failure preserves the current replacement and grants no output value.
     *  The output reference must not be retained by the implementation.
     *
     *  # Safety
     *  Call on the game thread with offline hooks active, after releasing old
     *  native references and with no concurrent native reads of the replacement.
     */
    int32_t (*prepare_monster_spawn)(Erased_t const *, QuestMonsterSpawn_t, uint32_t *);

    /** \brief
     *  A synchronized range query. It grants no pointer or lasting borrow, and
     *  the replacement may change after this call returns.
     */
    bool (*override_contains)(Erased_t const *, uint32_t, size_t);

    /** \brief
     *  Replace one primary quest spawn; preserves the replacement on failure.
     *  # Safety
     *  Call on the game thread before restarting the quest loader, with no
     *  concurrent readers of replacement data. Offsets refer to the current quest.
     */
    int32_t (*replace_monster)(Erased_t const *, uint32_t, uint8_t, uint8_t);
} QuestControlApiVTable_t;

/** <No documentation available> */
typedef struct VirtualPtr__Erased_ptr_QuestControlApiVTable {
    /** <No documentation available> */
    Erased_t * ptr;

    /** <No documentation available> */
    QuestControlApiVTable_t vtable;
} VirtualPtr__Erased_ptr_QuestControlApiVTable_t;

typedef VirtualPtr__Erased_ptr_QuestControlApiVTable_t QuestControlTable;

/** <No documentation available> */
typedef struct QuestLaunchApiVTable {
    /** <No documentation available> */
    void (*release_vptr)(Erased_t *);

    /** <No documentation available> */
    int32_t (*prepare_local)(Erased_t const *, slice_ref_uint8_t);
} QuestLaunchApiVTable_t;

/** <No documentation available> */
typedef struct VirtualPtr__Erased_ptr_QuestLaunchApiVTable {
    /** <No documentation available> */
    Erased_t * ptr;

    /** <No documentation available> */
    QuestLaunchApiVTable_t vtable;
} VirtualPtr__Erased_ptr_QuestLaunchApiVTable_t;

typedef VirtualPtr__Erased_ptr_QuestLaunchApiVTable_t QuestLaunchTable;

#define MHF_QUEST_PROVIDER "mhf.base"
#define MHF_QUEST_INTERFACE "mhf.quest.v1"
#define MHF_QUEST_CONTROL_INTERFACE "mhf.quest.control.v3"
#define MHF_QUEST_LAUNCH_INTERFACE "mhf.quest.launch.v1"
/** \brief
 *  Simplified for lighter documentation, but the actual impls
 *  range from `Tuple1` up to `Tuple6`.
 */
typedef struct Tuple2_bool_uint8 {
    /** <No documentation available> */
    bool _0;

    /** <No documentation available> */
    uint8_t _1;
} Tuple2_bool_uint8_t;

/** <No documentation available> */
typedef struct DebugSnapshot {
    /** <No documentation available> */
    bool ready;

    /** <No documentation available> */
    uint16_t quest_id;

    /** <No documentation available> */
    uint16_t area;

    /** <No documentation available> */
    Tuple2_bool_uint8_t monster;

    /** <No documentation available> */
    float_3_array_t position;

    /** <No documentation available> */
    uint32_t hit_checks;

    /** <No documentation available> */
    uint32_t hits;
} DebugSnapshot_t;

typedef DebugSnapshot_t DebugSnapshot;

/** <No documentation available> */
typedef struct DebugApiVTable {
    /** <No documentation available> */
    void (*release_vptr)(Erased_t *);

    /** <No documentation available> */
    DebugSnapshot_t (*snapshot)(Erased_t const *);

    /** <No documentation available> */
    int32_t (*transform)(Erased_t const *, uint8_t);

    /** <No documentation available> */
    int32_t (*restore_hunter)(Erased_t const *);

    /** <No documentation available> */
    int32_t (*change_area)(Erased_t const *, uint16_t);

    /** <No documentation available> */
    int32_t (*restart)(Erased_t const *);

    /** <No documentation available> */
    int32_t (*exit)(Erased_t const *);
} DebugApiVTable_t;

/** <No documentation available> */
typedef struct VirtualPtr__Erased_ptr_DebugApiVTable {
    /** <No documentation available> */
    Erased_t * ptr;

    /** <No documentation available> */
    DebugApiVTable_t vtable;
} VirtualPtr__Erased_ptr_DebugApiVTable_t;

typedef VirtualPtr__Erased_ptr_DebugApiVTable_t DebugTable;

#define MHF_DEBUG_PROVIDER "mhf.debug"
#define MHF_DEBUG_INTERFACE "mhf.debug-tools.v1"
/** <No documentation available> */
typedef struct UiApiVTable {
    /** <No documentation available> */
    void (*release_vptr)(Erased_t *);

    /** <No documentation available> */
    int32_t (*label)(Erased_t const *, slice_ref_uint8_t);

    /** \brief
     *  `OK` makes the returned button state valid.
     */
    int32_t (*button)(Erased_t const *, slice_ref_uint8_t, bool *);

    /** \brief
     *  `value` is read as the current state and updated on `OK`.
     */
    int32_t (*checkbox)(Erased_t const *, slice_ref_uint8_t, bool *);
} UiApiVTable_t;

/** <No documentation available> */
typedef struct VirtualPtr__Erased_ptr_UiApiVTable {
    /** <No documentation available> */
    Erased_t * ptr;

    /** <No documentation available> */
    UiApiVTable_t vtable;
} VirtualPtr__Erased_ptr_UiApiVTable_t;

typedef VirtualPtr__Erased_ptr_UiApiVTable_t UiTable;

typedef int32_t (*RenderFn)(void *, VirtualPtr__Erased_ptr_UiApiVTable_t const *);

/** <No documentation available> */
typedef struct UiHostApiVTable {
    /** <No documentation available> */
    void (*release_vptr)(Erased_t *);

    /** \brief
     *  Only `OK` makes the returned handle valid. C callers must provide valid
     *  UTF-8 with a non-null pointer even for an empty title, and a valid
     *  exclusive output reference.
     *
     *  # Safety
     *  Keep the table, object, and provider code live. `render`, its DLL, and
     *  `user` state must be ready before registration and remain valid until
     *  successful synchronous unregister or completed provider shutdown. They
     *  must support serialized UI-thread calls without unwinding; `user` may be
     *  null only if `render` supports it. Failure must leave no registered or
     *  in-flight callback and retain neither callback nor state pointer.
     */
    int32_t (*register_panel)(Erased_t const *, slice_ref_uint8_t, int32_t (*)(void *, VirtualPtr__Erased_ptr_UiApiVTable_t const *), void *, uint64_t *);

    /** \brief
     *  Call on the host lifecycle thread, never from this panel's render
     *  callback (which would wait for itself). `OK` removes the panel and waits
     *  for active callbacks; afterwards none may start. On failure, callbacks
     *  may remain active or start later: retain their state/code and propagate
     *  cleanup failure so the host keeps the consumer loaded.
     */
    int32_t (*unregister_panel)(Erased_t const *, uint64_t);
} UiHostApiVTable_t;

/** <No documentation available> */
typedef struct VirtualPtr__Erased_ptr_UiHostApiVTable {
    /** <No documentation available> */
    Erased_t * ptr;

    /** <No documentation available> */
    UiHostApiVTable_t vtable;
} VirtualPtr__Erased_ptr_UiHostApiVTable_t;

typedef VirtualPtr__Erased_ptr_UiHostApiVTable_t UiHostTable;

#define MHF_UI_PROVIDER "mhf.base"
#define MHF_UI_INTERFACE "mhf.ui.v1"

#ifdef __cplusplus
}
#endif
#endif /* MHF_GAME_H */
