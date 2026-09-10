/* A C host exercises three real shared libraries and the provider's public ABI.
 * This harness tests the ABI; dependency discovery belongs to the real host. */
#include "counter.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>

#ifdef _WIN32
#include <windows.h>
typedef HMODULE Library;
static Library open_library(const char *path) { return LoadLibraryA(path); }
static MhfModQueryV2 query_export(Library library) {
    return (MhfModQueryV2)GetProcAddress(library, "mhf_mod_query_v2");
}
static void close_library(Library library) { FreeLibrary(library); }
#else
#include <dlfcn.h>
typedef void *Library;
static Library open_library(const char *path) { return dlopen(path, RTLD_NOW | RTLD_LOCAL); }
static MhfModQueryV2 query_export(Library library) {
    return (MhfModQueryV2)dlsym(library, "mhf_mod_query_v2");
}
static void close_library(Library library) { dlclose(library); }
#endif

static const CounterTable *counter;
static uint32_t log_count;

static int equals(MhfStr input, const char *expected) {
    return input.len == strlen(expected) && memcmp(input.ptr, expected, input.len) == 0;
}

static void MHF_CALL log_message(void *context, uint32_t level, MhfStr message) {
    (void)context;
    assert(level == MHF_LOG_INFO);
    assert(message.len > 0);
    printf("%.*s\n", (int)message.len, (const char *)message.ptr);
    ++log_count;
}

static MhfStatus MHF_CALL empty_text(void *context, uint8_t *buffer,
                                    uint32_t capacity, uint32_t *required) {
    (void)context; (void)buffer; (void)capacity;
    *required = 0;
    return MHF_OK;
}

static MhfStatus MHF_CALL register_interface(void *context, MhfStr id, const void *table) {
    (void)context;
    assert(equals(id, EXAMPLE_COUNTER_INTERFACE));
    assert(counter == NULL);
    counter = (const CounterTable *)table;
    return MHF_OK;
}

static MhfStatus MHF_CALL dependency(void *context, MhfStr provider,
                                    MhfStr id, const void **table) {
    (void)context;
    assert(equals(provider, EXAMPLE_COUNTER_PROVIDER));
    assert(equals(id, EXAMPLE_COUNTER_INTERFACE));
    assert(counter != NULL);
    *table = counter;
    return MHF_OK;
}

static MhfStatus MHF_CALL game_info(void *context, MhfGameInfoV2 *out) {
    (void)context;
    out->module_base = NULL;
    out->phase = MHF_PHASE_ATTACH;
    return MHF_OK;
}

static MhfStatus MHF_CALL prepare_group(void *context, MhfStr name, void *state,
                                       MhfDrainFn drain, MhfHookGroup *out) {
    (void)context; (void)name; (void)state; (void)drain; (void)out;
    return MHF_INVALID_STATE;
}
static MhfStatus MHF_CALL create_hook(void *context, MhfHookGroup group, void *target,
                                     void *detour, void **out) {
    (void)context; (void)group; (void)target; (void)detour; (void)out;
    return MHF_INVALID_STATE;
}
static MhfStatus MHF_CALL change_group(void *context, MhfHookGroup group) {
    (void)context; (void)group;
    return MHF_INVALID_STATE;
}

static const MhfHostV2 HOST = {
    NULL, log_message, empty_text, empty_text, empty_text,
    register_interface, dependency, game_info,
    { prepare_group, create_hook, change_group, change_group }
};

static void run(char **paths) {
    Library libraries[3];
    const MhfModV2 *mods[3];
    void *instances[3] = {NULL, NULL, NULL};
    CounterSnapshot snapshot;
    int index;
    counter = NULL;
    log_count = 0;
    for (index = 0; index < 3; ++index) {
        MhfModQueryV2 query;
        libraries[index] = open_library(paths[index]);
        assert(libraries[index] != NULL);
        query = query_export(libraries[index]);
        assert(query != NULL);
        mods[index] = query();
        assert(mods[index]->create(&HOST, &instances[index]) == MHF_OK);
    }
    for (index = 0; index < 3; ++index) {
        if (mods[index]->prepare) assert(mods[index]->prepare(instances[index]) == MHF_OK);
    }
    for (index = 0; index < 3; ++index) {
        if (mods[index]->check) assert(mods[index]->check(instances[index]) == MHF_OK);
    }
    for (index = 0; index < 3; ++index) {
        assert(mods[index]->attach(instances[index]) == MHF_OK);
    }
    assert(log_count == 2);
    snapshot = counter->vtable.snapshot(counter->ptr);
    assert(snapshot.count == 11);
    assert(counter->vtable.add(counter->ptr, UINT32_MAX) == MHF_ERROR);
    snapshot = counter->vtable.snapshot(counter->ptr);
    assert(snapshot.count == 11);
    for (index = 2; index >= 0; --index) {
        if (mods[index]->stop) assert(mods[index]->stop(instances[index]) == MHF_OK);
        if (mods[index]->detach) assert(mods[index]->detach(instances[index]) == MHF_OK);
        mods[index]->destroy(instances[index]);
        close_library(libraries[index]);
    }
}

int main(int argc, char **argv) {
    int round;
    if (argc != 4) {
        fprintf(stderr, "usage: abi-smoke <provider> <rust-consumer> <c-consumer>\n");
        return 2;
    }
    for (round = 0; round < 2; ++round) {
        run(argv + 1);
    }
    puts("C ABI smoke passed twice: Rust and C consumers share the provider, count = 11");
    return 0;
}
