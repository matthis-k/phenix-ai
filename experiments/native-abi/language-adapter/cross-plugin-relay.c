/* Native consumer plugin .so2 that calls the Python adapter plugin .so1.
 * This directly invokes the contract for experiment purposes; production
 * Phenix must mediate all calls through selected authority/grant checks.
 */
#include "../include/phenix_native_experiment.h"
#include <dlfcn.h>
#include <stdint.h>
#include <stdlib.h>

typedef struct {
    void *library;
    PhenixOpaqueObject downstream;
} RelayState;

static uint32_t relay_invoke(void *opaque, PhenixRequest request, PhenixResponse *response) {
    RelayState *state = opaque;
    if (!state || !state->downstream.context) return 1;
    return state->downstream.invoke(state->downstream.context, request, response);
}

static void relay_destroy(void *opaque) {
    RelayState *state = opaque;
    if (!state) return;
    state->downstream.destroy(state->downstream.context);
    dlclose(state->library);
    free(state);
}

static PhenixOpaqueObject relay_create(void) {
    PhenixOpaqueObject empty = {0};
    const char *target = getenv("PHENIX_PY_ADAPTER_SO");
    if (!target || !*target) return empty;
    void *lib = dlopen(target, RTLD_NOW | RTLD_LOCAL);
    if (!lib) return empty;
    PhenixEntry entry = (PhenixEntry)dlsym(lib, "phenix_native_experiment_v1");
    if (!entry) { dlclose(lib); return empty; }
    const PhenixPublicApi *api = entry();
    if (!api || api->header.version != PHENIX_ABI_VERSION
        || api->header.table_bytes != sizeof(*api)
        || api->header.schema != PHENIX_SCHEMA_ID || !api->create) {
        dlclose(lib);
        return empty;
    }
    PhenixOpaqueObject downstream = api->create();
    if (!downstream.context || !downstream.invoke || !downstream.destroy) {
        dlclose(lib);
        return empty;
    }
    RelayState *state = calloc(1, sizeof(*state));
    if (!state) {
        downstream.destroy(downstream.context);
        dlclose(lib);
        return empty;
    }
    state->library = lib;
    state->downstream = downstream;
    return (PhenixOpaqueObject) {
        .context = state, .invoke = relay_invoke, .destroy = relay_destroy,
    };
}

static const PhenixPublicApi api = {
    .header = {
        .version = PHENIX_ABI_VERSION,
        .table_bytes = sizeof(PhenixPublicApi),
        .schema = PHENIX_SCHEMA_ID,
    },
    .create = relay_create,
};

__attribute__((visibility("default")))
const PhenixPublicApi *phenix_native_experiment_v1(void) { return &api; }
