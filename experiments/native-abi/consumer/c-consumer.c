#include "../include/phenix_native_experiment.h"
#include <assert.h>
#include <dlfcn.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

static void run(const char *path) {
    void *library = dlopen(path, RTLD_NOW | RTLD_LOCAL);
    if (!library) { fprintf(stderr, "dlopen %s: %s\n", path, dlerror()); assert(library); }
    PhenixEntry entry = (PhenixEntry)dlsym(library, "phenix_native_experiment_v1");
    assert(entry);
    const PhenixPublicApi *api = entry();
    assert(api);
    assert(api->header.version == PHENIX_ABI_VERSION);
    assert(api->header.table_bytes == sizeof(*api));
    assert(api->header.schema == PHENIX_SCHEMA_ID);
    PhenixOpaqueObject object = api->create();
    assert(object.context && object.invoke && object.destroy);
    static const uint8_t words[] = "one two three";
    PhenixRequest request = {
        .query = { words, sizeof(words) - 1 },
        .options = { .multiplier = 7, .maximum = 2, .mode = 0 },
    };
    PhenixResponse response = {0};
    assert(object.invoke(object.context, request, &response) == 0);
    assert(response.score == 15 && response.calls == 1);
    assert(object.invoke(object.context, request, &response) == 0);
    assert(response.score == 16 && response.calls == 2);
    request.options.mode = 123;
    assert(object.invoke(object.context, request, &response) == 1);
    object.destroy(object.context);
    assert(dlclose(library) == 0);
}

int main(int argc, char **argv) {
    assert(argc == 3);
    assert(sizeof(PhenixApiHeader) == 16);
    assert(sizeof(PhenixByteSlice) == 16);
    assert(sizeof(PhenixPublicApi) == 24);
    assert(sizeof(PhenixOpaqueObject) == 24);
    run(argv[1]);
    run(argv[2]);
    puts("C language adapter smoke: both independently compiled Rust providers passed");
    return 0;
}
