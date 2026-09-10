/* Generated from Rust by safer-ffi. Do not edit. */
#ifndef MHF_MOD_H
#define MHF_MOD_H

/* Host interface lookups return borrowed pointers. Do not free them or
 * consume/copy a generated virtual object as an owner; never call its
 * vtable.release_vptr. Calls and borrowed results must end before provider
 * unloading. Frame callbacks may further limit the borrow. Implementations
 * and callbacks must not unwind across the C ABI. */

#ifdef __cplusplus
extern "C" {
#endif


#include <stddef.h>
#include <stdint.h>

typedef int32_t MhfStatus;

/** \brief
 *  UTF-8 borrowed for one call. No terminating NUL; a zero length permits NULL.
 */
typedef struct Str {
    /** <No documentation available> */
    uint8_t const * ptr;

    /** <No documentation available> */
    uint32_t len;
} Str_t;

typedef Str_t MhfStr;

/** \brief
 *  `&'lt [T]` but with a guaranteed `#[repr(C)]` layout.
 *
 *  # C layout (for some given type T)
 *
 *  ```c
 *  typedef struct {
 *  // Cannot be NULL
 *  T * ptr;
 *  size_t len;
 *  } slice_T;
 *  ```
 *
 *  # Nullable pointer?
 *
 *  If you want to support the above typedef, but where the `ptr` field is
 *  allowed to be `NULL` (with the contents of `len` then being undefined)
 *  use the `Option< slice_ptr<_> >` type.
 */
typedef struct slice_ref_uint8 {
    /** \brief
     *  Pointer to the first element (if any).
     */
    uint8_t const * ptr;

    /** \brief
     *  Element count
     */
    size_t len;
} slice_ref_uint8_t;

typedef slice_ref_uint8_t MhfUtf8;

/** \brief
 *  `&'lt mut [T]` but with a guaranteed `#[repr(C)]` layout.
 *
 *  # C layout (for some given type T)
 *
 *  ```c
 *  typedef struct {
 *  // Cannot be NULL
 *  T * ptr;
 *  size_t len;
 *  } slice_T;
 *  ```
 *
 *  # Nullable pointer?
 *
 *  If you want to support the above typedef, but where the `ptr` field is
 *  allowed to be `NULL` (with the contents of `len` then being undefined)
 *  use the `Option< slice_ptr<_> >` type.
 */
typedef struct slice_mut_uint8 {
    /** \brief
     *  Pointer to the first element (if any).
     */
    uint8_t * ptr;

    /** \brief
     *  Element count
     */
    size_t len;
} slice_mut_uint8_t;

typedef slice_mut_uint8_t MhfMutBytes;

/** \brief
 *  `module_base` is borrowed from the host and NULL before the game DLL loads.
 *  A mod must not keep an independent DLL reference beyond detach; the host
 *  coordinates the final release of game and mod libraries.
 */
typedef struct GameInfoV2 {
    /** <No documentation available> */
    void * module_base;

    /** <No documentation available> */
    uint32_t phase;
} GameInfoV2_t;

typedef GameInfoV2_t MhfGameInfoV2;

typedef struct {
    uint8_t idx[8];
} uint8_8_array_t;

typedef struct {
    uint8_t idx[1024];
} uint8_1024_array_t;

typedef uint8_t MhfGameBytes2048[2048];

typedef struct {
    uint8_t idx[64];
} uint8_64_array_t;

typedef struct {
    uint8_t idx[16];
} uint8_16_array_t;

typedef struct {
    uint8_t idx[512];
} uint8_512_array_t;

typedef struct {
    uint8_t idx[256];
} uint8_256_array_t;

typedef struct {
    uint32_t idx[16];
} uint32_16_array_t;

typedef uint8_t MhfGameBytes104[104];

/** <No documentation available> */
typedef struct LaunchParams32 {
    /** <No documentation available> */
    uint32_t module_instance;

    /** <No documentation available> */
    uint8_8_array_t unknown_0004;

    /** <No documentation available> */
    uint32_t launch_mode;

    /** <No documentation available> */
    uint32_t launch_flags;

    /** <No documentation available> */
    uint8_1024_array_t game_dir;

    /** <No documentation available> */
    uint8_1024_array_t launcher_dir;

    /** <No documentation available> */
    MhfGameBytes2048 username;

    /** <No documentation available> */
    MhfGameBytes2048 password;

    /** <No documentation available> */
    uint32_t command_number;

    /** <No documentation available> */
    uint32_t command_netcf_update;

    /** <No documentation available> */
    uint32_t command_dmm;

    /** <No documentation available> */
    uint32_t mhf_mutex_number;

    /** <No documentation available> */
    uint32_t instance_mutex;

    /** <No documentation available> */
    uint32_t master_ready_mutex;

    /** <No documentation available> */
    uint8_64_array_t mutex_name;

    /** <No documentation available> */
    uint8_64_array_t ini_name;

    /** <No documentation available> */
    uint32_t host_callback_release;

    /** <No documentation available> */
    uint32_t host_callback_state;

    /** <No documentation available> */
    uint32_t host_callback_query;

    /** <No documentation available> */
    uint32_t host_callback_result;

    /** <No documentation available> */
    uint32_t status_code;

    /** <No documentation available> */
    uint32_t error_code;

    /** <No documentation available> */
    uint32_t selected_character_id_1;

    /** <No documentation available> */
    uint32_t selected_character_id_2;

    /** <No documentation available> */
    uint32_t sign_session_id;

    /** <No documentation available> */
    uint8_16_array_t sign_session_token;

    /** <No documentation available> */
    uint8_8_array_t reserved_18e0;

    /** <No documentation available> */
    uint32_t sign_session_issued_at;

    /** <No documentation available> */
    uint32_t fixed_18ec_zero;

    /** <No documentation available> */
    uint8_512_array_t reserved_18f0;

    /** <No documentation available> */
    uint8_256_array_t entrance_server_address;

    /** <No documentation available> */
    uint8_256_array_t entrance_server_host;

    /** <No documentation available> */
    uint32_t patch_server_count;

    /** <No documentation available> */
    uint32_t entrance_server_count;

    /** <No documentation available> */
    uint32_t selected_character_status;

    /** <No documentation available> */
    uint32_t course_rights;

    /** <No documentation available> */
    uint32_t selected_character_hr;

    /** <No documentation available> */
    uint8_16_array_t selected_character_name;

    /** <No documentation available> */
    uint32_16_array_t character_ids;

    /** <No documentation available> */
    uint32_t global_alloc;

    /** <No documentation available> */
    uint32_t fixed_1d58_one;

    /** <No documentation available> */
    uint32_t unknown_1d5c;

    /** <No documentation available> */
    uint32_t selected_character_gr;

    /** <No documentation available> */
    uint8_8_array_t reserved_1d64;

    /** <No documentation available> */
    uint32_t preset_level;

    /** <No documentation available> */
    uint32_t custom;

    /** <No documentation available> */
    uint32_t screen_mode;

    /** <No documentation available> */
    uint32_t window_width;

    /** <No documentation available> */
    uint32_t window_height;

    /** <No documentation available> */
    uint32_t fullscreen_width;

    /** <No documentation available> */
    uint32_t fullscreen_height;

    /** <No documentation available> */
    uint32_t display_character_limit;

    /** <No documentation available> */
    uint32_t use_dxt_textures;

    /** <No documentation available> */
    uint32_t now_monitor_wh;

    /** <No documentation available> */
    uint32_t graphics_version;

    /** <No documentation available> */
    uint32_t sound_disabled;

    /** <No documentation available> */
    uint32_t sound_volume;

    /** <No documentation available> */
    uint32_t inactive_sound_volume;

    /** <No documentation available> */
    uint32_t minimized_sound_volume;

    /** <No documentation available> */
    uint32_t sound_sample_rate;

    /** <No documentation available> */
    uint32_t sound_buffer_size;

    /** <No documentation available> */
    uint32_t language;

    /** <No documentation available> */
    uint32_t font_quality;

    /** <No documentation available> */
    uint32_t font_weight;

    /** <No documentation available> */
    MhfGameBytes104 font_name;

    /** <No documentation available> */
    uint32_t draw_skip;

    /** <No documentation available> */
    uint32_t clog_disabled;

    /** <No documentation available> */
    uint32_t use_proxy;

    /** <No documentation available> */
    uint32_t use_ie_proxy;

    /** <No documentation available> */
    uint32_t proxy_configured;

    /** <No documentation available> */
    uint8_64_array_t proxy_address;

    /** <No documentation available> */
    uint32_t proxy_port;

    /** <No documentation available> */
    uint32_t server_selection;

    /** <No documentation available> */
    uint32_t host_services;

    /** <No documentation available> */
    uint8_64_array_t reserved_1e84;

    /** <No documentation available> */
    uint8_64_array_t reserved_1ec4;

    /** <No documentation available> */
    uint8_256_array_t alternate_entrance_server_address;

    /** <No documentation available> */
    uint32_t return_expires_at;

    /** <No documentation available> */
    uint32_t unknown_2008;

    /** <No documentation available> */
    uint32_t fixed_200c_one;
} LaunchParams32_t;

typedef LaunchParams32_t MhfLaunchParams32;

typedef uint8_t MhfGameBytes2572[2572];

typedef struct {
    uint32_t idx[4];
} uint32_4_array_t;

typedef struct {
    uint16_t idx[4];
} uint16_4_array_t;

typedef uint8_t MhfGameBytes4096[4096];

typedef struct {
    MhfGameBytes4096 idx[4];
} MhfGameBytes4096_4_array_t;

typedef uint8_t MhfGameBytes16512[16512];

typedef struct {
    uint32_t idx[8];
} uint32_8_array_t;

/** <No documentation available> */
typedef struct GlobalData32 {
    /** <No documentation available> */
    MhfGameBytes2572 reserved_0000;

    /** <No documentation available> */
    uint32_4_array_t notice_lengths;

    /** <No documentation available> */
    uint8_8_array_t reserved_0a1c;

    /** <No documentation available> */
    uint16_4_array_t notice_flags;

    /** <No documentation available> */
    MhfGameBytes4096_4_array_t notices;

    /** <No documentation available> */
    MhfGameBytes16512 reserved_4a2c;

    /** <No documentation available> */
    uint32_t festa_id;

    /** <No documentation available> */
    uint32_t festa_starts_at;

    /** <No documentation available> */
    uint32_t festa_expires_at;

    /** <No documentation available> */
    uint32_t festa_solo_tickets;

    /** <No documentation available> */
    uint32_t festa_group_tickets;

    /** <No documentation available> */
    uint32_8_array_t festa_stalls;
} GlobalData32_t;

typedef GlobalData32_t MhfGlobalData32;

/** \brief
 *  Host-owned, initialized launch storage. Both pointers are valid and exclusive
 *  for one launch callback. A provider must not retain either pointer.
 */
typedef struct LaunchTargetV1 {
    /** <No documentation available> */
    LaunchParams32_t * params;

    /** <No documentation available> */
    GlobalData32_t * global;
} LaunchTargetV1_t;

typedef LaunchTargetV1_t MhfLaunchTargetV1;

/** \brief
 *  Mods publish this table during prepare under the launch or fallback launch
 *  interface ID. The host calls one provider from the launch tier, or from the
 *  fallback tier when no launch provider exists. The chosen tier must have one
 *  provider. Calls run once before game loading, on the lifecycle thread. Return OK
 *  when launch data is ready, CANCELLED for user cancellation, or an error after
 *  logging details through the host. The callback must not unwind or retain the
 *  target pointers. The provider owns this table and its state through destroy.
 */
typedef struct LaunchApiV1 {
    /** <No documentation available> */
    void * context;

    /** <No documentation available> */
    int32_t (*run)(void *, LaunchTargetV1_t *);
} LaunchApiV1_t;

typedef LaunchApiV1_t MhfLaunchApiV1;

typedef void * MhfHookGroup;

typedef int32_t (*MhfDrainFn)(void *);

typedef int32_t (*MhfLifecycleFn)(void *);

typedef int32_t (*MhfReadTextFn)(void *, uint8_t *, uint32_t, uint32_t *);

/** \brief
 *  The host owns all groups. The mod owns callback state until group cleanup
 *  succeeds. `drain` runs after hooks are disabled and before trampolines vanish.
 */
typedef struct HookApiV1 {
    /** <No documentation available> */
    int32_t (*prepare_group)(void *, Str_t, void *, int32_t (*)(void *), void * *);

    /** <No documentation available> */
    int32_t (*create_hook)(void *, void *, void *, void *, void * *);

    /** <No documentation available> */
    int32_t (*enable_group)(void *, void *);

    /** <No documentation available> */
    int32_t (*discard_group)(void *, void *);
} HookApiV1_t;

typedef HookApiV1_t MhfHookApiV1;

/** \brief
 *  A stable per-mod host table, borrowed until that mod's destroy returns.
 *  Host operations run on the lifecycle thread. Provider interfaces can define
 *  their own threading rules. No host lock is held while invoking mod code.
 */
typedef struct HostV2 {
    /** <No documentation available> */
    void * context;

    /** <No documentation available> */
    void (*log)(void *, uint32_t, Str_t);

    /** \brief
     *  UTF-8 TOML for this mod's settings. Output excludes a terminating NUL.
     */
    int32_t (*config)(void *, uint8_t *, uint32_t, uint32_t *);

    /** \brief
     *  Absolute UTF-8 package resource directory.
     */
    int32_t (*resource_root)(void *, uint8_t *, uint32_t, uint32_t *);

    /** \brief
     *  Last host error for this mod. Reading it must not clear or replace it.
     */
    int32_t (*last_error)(void *, uint8_t *, uint32_t, uint32_t *);

    /** \brief
     *  The table and anything it references stay alive until destroy. Interfaces
     *  registered during prepare or attach become available once that phase
     *  succeeds for this provider.
     */
    int32_t (*register_interface)(void *, Str_t, void const *);

    /** \brief
     *  This Mod's published interfaces and declared dependencies are visible.
     *  A successful table remains valid
     *  through this consumer's destroy, including stop and detach. It is borrowed:
     *  consumers must not free it or consume/copy an owning virtual object from it.
     *  In particular, never call a safer-ffi object's vtable.release_vptr.
     */
    int32_t (*dependency)(void *, Str_t, Str_t, void const * *);

    /** <No documentation available> */
    int32_t (*game_info)(void *, GameInfoV2_t *);

    /** <No documentation available> */
    HookApiV1_t hooks;
} HostV2_t;

typedef HostV2_t MhfHostV2;

/** \brief
 *  Lifecycle calls are serialized. All callbacks must contain panics/exceptions.
 *  `create` owns allocation and `destroy` frees it in the same DLL. Failed
 *  stop/detach/drain leaves the instance, DLL, and its dependencies resident.
 */
typedef struct ModV2 {
    /** <No documentation available> */
    int32_t (*create)(HostV2_t const *, void * *);

    /** <No documentation available> */
    int32_t (*prepare)(void *);

    /** <No documentation available> */
    int32_t (*check)(void *);

    /** <No documentation available> */
    int32_t (*attach)(void *);

    /** <No documentation available> */
    int32_t (*stop)(void *);

    /** <No documentation available> */
    int32_t (*detach)(void *);

    /** <No documentation available> */
    void (*destroy)(void *);
} ModV2_t;

typedef ModV2_t MhfModV2;

typedef ModV2_t const * (*MhfModQueryV2)(void);

#define MHF_OK ((int32_t) 0)
#define MHF_ERROR ((int32_t) 1)
#define MHF_BUFFER_TOO_SMALL ((int32_t) 2)
#define MHF_NOT_FOUND ((int32_t) 3)
#define MHF_INVALID_STATE ((int32_t) 4)
#define MHF_CONFLICT ((int32_t) 5)
#define MHF_CANCELLED ((int32_t) 6)
#define MHF_LOG_ERROR ((uint32_t) 0)
#define MHF_LOG_WARN ((uint32_t) 1)
#define MHF_LOG_INFO ((uint32_t) 2)
#define MHF_LOG_DEBUG ((uint32_t) 3)
#define MHF_LOG_TRACE ((uint32_t) 4)
#define MHF_PHASE_PREPARE ((uint32_t) 0)
#define MHF_PHASE_CHECK ((uint32_t) 1)
#define MHF_PHASE_ATTACH ((uint32_t) 2)
#define MHF_PHASE_RUNNING ((uint32_t) 3)
#define MHF_PHASE_STOP ((uint32_t) 4)
#define MHF_PHASE_DETACH ((uint32_t) 5)
#define MHF_PHASE_DESTROY ((uint32_t) 6)
#define MHF_LAUNCH_INTERFACE_ID "mhf.launch.v1"
#define MHF_FALLBACK_LAUNCH_INTERFACE_ID "mhf.launch.fallback.v1"

#if defined(_WIN32)
#define MHF_CALL __cdecl
#define MHF_EXPORT __declspec(dllexport)
#else
#define MHF_CALL
#define MHF_EXPORT __attribute__((visibility("default")))
#endif
#define MHF_STR_LITERAL(value) { (const uint8_t *)(value), sizeof(value) - 1u }

#ifdef __cplusplus
}
#endif
#endif /* MHF_MOD_H */
