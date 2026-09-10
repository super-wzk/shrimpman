#include "counter.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct Consumer {
    const MhfHostV2 *host;
    const CounterTable *counter;
} Consumer;

static MhfStatus MHF_CALL create(const MhfHostV2 *host, void **out_instance) {
    Consumer *consumer = (Consumer *)calloc(1, sizeof(Consumer));
    if (consumer == NULL) return MHF_ERROR;
    consumer->host = host;
    *out_instance = consumer;
    return MHF_OK;
}

static MhfStatus MHF_CALL attach(void *instance) {
    Consumer *consumer = (Consumer *)instance;
    const MhfStr provider = MHF_STR_LITERAL(EXAMPLE_COUNTER_PROVIDER);
    const MhfStr interface_id = MHF_STR_LITERAL(EXAMPLE_COUNTER_INTERFACE);
    const void *table = NULL;
    CounterSnapshot snapshot;
    char text[80];
    MhfStr message;
    MhfStatus status = consumer->host->dependency(
        consumer->host->context, provider, interface_id, &table);
    if (status != MHF_OK) return status;
    consumer->counter = (const CounterTable *)table;
    status = consumer->counter->vtable.add(consumer->counter->ptr, 10);
    if (status != MHF_OK) return status;
    snapshot = consumer->counter->vtable.snapshot(consumer->counter->ptr);
    snprintf(text, sizeof(text), "C consumer: counter = %u", (unsigned)snapshot.count);
    message.ptr = (const uint8_t *)text;
    message.len = (uint32_t)strlen(text);
    consumer->host->log(consumer->host->context, MHF_LOG_INFO, message);
    return MHF_OK;
}

static MhfStatus MHF_CALL stop(void *instance) {
    ((Consumer *)instance)->counter = NULL;
    return MHF_OK;
}

static void MHF_CALL destroy(void *instance) {
    free(instance);
}

static const MhfModV2 MOD = { create, NULL, NULL, attach, stop, NULL, destroy };

MHF_EXPORT const MhfModV2 *MHF_CALL mhf_mod_query_v2(void) {
    return &MOD;
}
