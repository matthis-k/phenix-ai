#define _POSIX_C_SOURCE 200809L
#include "../include/phenix_native_experiment.h"
#include <assert.h>
#include <dlfcn.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
    void *library;
    const PhenixPublicApi *api;
} Loaded;

static Loaded load(const char *path) {
    Loaded loaded = {0};
    loaded.library = dlopen(path, RTLD_NOW | RTLD_LOCAL);
    if (!loaded.library) {
        fprintf(stderr, "dlopen(%s): %s\n", path, dlerror());
        abort();
    }
    PhenixEntry entry = (PhenixEntry)dlsym(loaded.library, "phenix_native_experiment_v1");
    assert(entry);
    loaded.api = entry();
    assert(loaded.api);
    return loaded;
}

static void close_loaded(Loaded loaded) {
    assert(dlclose(loaded.library) == 0);
}

static PhenixRequest request(const char *text) {
    PhenixRequest result = {
        .query = {(const uint8_t *)text, strlen(text)},
        .options = { .multiplier = 7, .maximum = 2, .mode = 0 },
    };
    return result;
}

static PhenixResponse positive(PhenixOpaqueObject object, const char *text, int64_t expected_score,
                               uint64_t expected_calls) {
    PhenixResponse response = {0};
    assert(object.context && object.invoke && object.destroy);
    assert(object.invoke(object.context, request(text), &response) == 0);
    assert(response.score == expected_score);
    assert(response.calls == expected_calls);
    assert(response.status == 0);
    return response;
}

int main(int argc, char **argv) {
    assert(argc == 6);
    const char *relay_path = argv[1];
    const char *python_adapter_path = argv[2];
    assert(setenv("PHENIX_PY_ADAPTER_SO", python_adapter_path, 1) == 0);
    assert(setenv("PHENIX_PYTHON_PLUGIN_MODULE", "phenix_plugin_v1", 1) == 0);

    Loaded relay = load(relay_path);
    assert(relay.api->header.version == PHENIX_ABI_VERSION);
    assert(relay.api->header.table_bytes == sizeof(PhenixPublicApi));
    assert(relay.api->header.schema == PHENIX_SCHEMA_ID);
    PhenixOpaqueObject old = relay.api->create();
    PhenixOpaqueObject separate = relay.api->create();
    positive(old, "alpha beta gamma", 15, 1);
    positive(old, "one", 9, 2);
    positive(separate, "one two", 15, 1);

    /* Error boundary: malformed mode, UTF-8, plugin exception,
       invalid result type and out-of-range numeric result. */
    PhenixResponse response = {0};
    PhenixRequest wrong_mode = request("alpha");
    wrong_mode.options.mode = 99;
    assert(old.invoke(old.context, wrong_mode, &response) == 1);
    const uint8_t invalid_utf8[] = {0xff};
    PhenixRequest invalid = request("anything");
    invalid.query.ptr = invalid_utf8;
    invalid.query.len = sizeof(invalid_utf8);
    assert(old.invoke(old.context, invalid, &response) == 1);
    assert(old.invoke(old.context, request("raise"), &response) == 2);
    assert(old.invoke(old.context, request("wrong-type"), &response) == 2);
    assert(old.invoke(old.context, request("overflow"), &response) == 2);
    positive(old, "one two", 17, 3); /* failures must not increment Python state */

    /* Promote the Python implementation while retaining old object state. */
    assert(setenv("PHENIX_PYTHON_PLUGIN_MODULE", "phenix_plugin_v2", 1) == 0);
    PhenixOpaqueObject next = relay.api->create();
    positive(next, "one", 108, 1);
    positive(old, "one", 11, 4);

    /* Import failure must not expose a half-constructed object. */
    assert(setenv("PHENIX_PYTHON_PLUGIN_MODULE", "definitely_not_a_python_module", 1) == 0);
    PhenixOpaqueObject missing_module = relay.api->create();
    assert(missing_module.context == NULL);

    /* Missing adapter or ABI mismatch must not enter the provider. */
    assert(setenv("PHENIX_PY_ADAPTER_SO", "/missing/adapter.so", 1) == 0);
    assert(relay.api->create().context == NULL);
    for (int n = 3; n <= 5; ++n) {
        assert(setenv("PHENIX_PY_ADAPTER_SO", argv[n], 1) == 0);
        assert(relay.api->create().context == NULL);
    }

    /* Direct loader refuses mismatched version, struct size and schema. */
    for (int n = 3; n <= 5; ++n) {
        Loaded bad = load(argv[n]);
        assert(
            bad.api->header.version != PHENIX_ABI_VERSION
            || bad.api->header.table_bytes != sizeof(PhenixPublicApi)
            || bad.api->header.schema != PHENIX_SCHEMA_ID
        );
        close_loaded(bad);
    }

    positive(next, "one two", 116, 2); /* old and new generations still live */
    next.destroy(next.context);
    old.destroy(old.context);
    separate.destroy(separate.context);
    close_loaded(relay);

    puts("PASS: native .so2 -> Python adapter .so1 -> Python plugin; success and failure; state, ABI, promotion");
    return 0;
}
