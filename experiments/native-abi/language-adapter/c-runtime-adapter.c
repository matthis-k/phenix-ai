/* A non-Rust language adapter shared library. The stable public table stays
 * identical to the two Rust provider implementations; the private runtime
 * representation is C. Python/Zig/C++ adapters can own their own runtimes and
 * translate to this contract without exposing their internal objects. */
#include "../include/phenix_native_experiment.h"
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <limits.h>

typedef struct { uint64_t calls; } CPrivateState;

static uint32_t adapter_invoke(void *opaque, PhenixRequest request, PhenixResponse *response) {
    if (!opaque || !response || request.options.mode != 0
        || request.query.len > 4096
        || (request.query.len != 0 && !request.query.ptr)) return 1;
    int64_t words = 0;
    int inside = 0;
    for (size_t i = 0; i < request.query.len; ++i) {
        uint8_t ch = request.query.ptr[i];
        if (ch >= 128) return 1; /* This C bridge's public example supports ASCII. */
        int space = (ch == ' ' || ch == '\n' || ch == '\t' || ch == '\r');
        if (!space && !inside) ++words;
        inside = !space;
    }
    if ((uint64_t)words > request.options.maximum) words = request.options.maximum;
    /* A deliberately simple, bounded implementation avoiding signed overflow. */
    if (request.options.multiplier != 0
        && ((words > 0 && request.options.multiplier > INT64_MAX / words)
            || (words > 0 && request.options.multiplier < INT64_MIN / words))) return 1;
    CPrivateState *state = (CPrivateState *)opaque;
    ++state->calls;
    response->score = words * request.options.multiplier + (int64_t)state->calls;
    response->calls = state->calls;
    response->status = 0;
    response->reserved = 0;
    return 0;
}

static void adapter_destroy(void *instance) { free(instance); }

static PhenixOpaqueObject adapter_create(void) {
    CPrivateState *state = calloc(1, sizeof(*state));
    PhenixOpaqueObject object = {
        .context = state,
        .invoke = adapter_invoke,
        .destroy = adapter_destroy,
    };
    return object;
}

static const PhenixPublicApi api = {
    .header = {
        .version = PHENIX_ABI_VERSION,
        .table_bytes = sizeof(PhenixPublicApi),
        .schema = PHENIX_SCHEMA_ID,
    },
    .create = adapter_create,
};

__attribute__((visibility("default")))
const PhenixPublicApi *phenix_native_experiment_v1(void) { return &api; }
