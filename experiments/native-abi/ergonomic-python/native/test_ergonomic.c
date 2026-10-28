#define _POSIX_C_SOURCE 200809L
#include "../include/phenix_poc_abi.h"
#include <assert.h>
#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct { int calls, denied; } KernelMock;
static uint32_t authorized_invoke(void *context, uint64_t scope, PhxSlice operation,
                                  PhxSlice request, PhxReply *reply) {
    KernelMock *mock = context;
    if (scope != 0x51 || operation.size != strlen("example.index/search")
        || memcmp(operation.data, "example.index/search", operation.size)) {
        ++mock->denied;
        return 3;
    }
    /* This is a mock selected-service callback, not the real Phenix kernel. */
    if (request.size == 0 || request.size > 2048 || !memchr(request.data, 'h', request.size)) {
        ++mock->denied;
        return 3;
    }
    ++mock->calls;
    const char *body = "{\"matches\":[\"native-match\",\"second\"]}";
    char *bytes = malloc(strlen(body));
    assert(bytes);
    memcpy(bytes, body, strlen(body));
    *reply = (PhxReply){.status = 0, .body = {(uint8_t *)bytes, strlen(body), free}};
    return 0;
}
static PhxSlice str(const char *s) { return (PhxSlice){(const uint8_t *)s, strlen(s)}; }
static void run(const PhxTable *api, void *instance, const PhxHost *host,
                const char *method, const char *request, uint32_t want, const char *contains) {
    PhxReply reply = {0};
    uint32_t code = api->invoke(instance, host, str(method), str(request), &reply);
    if (code != want || reply.status != want || !reply.body.release || !reply.body.data) {
        fprintf(stderr, "%s: expected status %u but got return %u body %u\n", method, want, code, reply.status);
        abort();
    }
    char *result = calloc(reply.body.size + 1, 1);
    assert(result);
    memcpy(result, reply.body.data, reply.body.size);
    if (!strstr(result, contains)) {
        fprintf(stderr, "%s: expected %s in %s\n", method, contains, result);
        abort();
    }
    reply.body.release(reply.body.data);
    free(result);
}
int main(int argc, char **argv) {
    assert(argc == 2);
    void *library = dlopen(argv[1], RTLD_NOW | RTLD_LOCAL);
    if (!library) { fprintf(stderr, "dlopen failed %s\n", dlerror()); abort(); }
    PhxEntry entry = (PhxEntry)dlsym(library, "phenix_ergonomic_poc_v1");
    assert(entry);
    const PhxTable *api = entry();
    assert(api && api->header.version == 1 && api->header.table_size == sizeof(PhxTable)
        && api->header.schema == PHENIX_POC_SCHEMA);
    KernelMock mock = {0};
    PhxHost host = {PHENIX_POC_VERSION, sizeof(PhxHost), 0x51, &mock, authorized_invoke};
    void *one = api->create(str("demo_search"), str("Search"));
    void *two = api->create(str("demo_search"), str("Search"));
    assert(one && two);

    PhxReply metadata = {0};
    assert(api->describe(one, &metadata) == 0 && metadata.body.release);
    char *manifest = calloc(metadata.body.size + 1, 1);
    assert(manifest);
    memcpy(manifest, metadata.body.data, metadata.body.size);
    assert(strstr(manifest, "example.search"));
    assert(strstr(manifest, "enriched_query"));
    assert(strstr(manifest, "record"));
    assert(!strstr(manifest, "private_method"));
    free(manifest);
    metadata.body.release(metadata.body.data);

    run(api, one, &host, "query", "{\"text\":\"hello world now\",\"limit\":2}", 0, "\"calls\":1");
    run(api, one, &host, "query", "{\"text\":\"again\",\"limit\":2}", 0, "\"calls\":2");
    run(api, two, &host, "query", "{\"text\":\"hello\",\"limit\":1}", 0, "\"calls\":1");
    run(api, one, &host, "enriched_query", "{\"text\":\"hello\",\"limit\":1}", 0, "native-match");
    assert(mock.calls == 1);
    run(api, one, &host, "enriched_query", "{\"text\":\"denied\",\"limit\":1}", 3, "denied");
    assert(mock.denied == 1);

    run(api, one, &host, "private_method", "{}", 1, "not publicly exported");
    run(api, one, &host, "query", "{\"text\":1,\"limit\":2}", 1, "expected str");
    run(api, one, &host, "query", "{\"text\":\"x\",\"limit\":true}", 1, "expected int");
    run(api, one, &host, "query", "{\"text\":\"x\",\"limit\":2,\"hack\":1}", 1, "missing or unknown fields");
    run(api, one, &host, "query", "not-json", 1, "schema_rejected");
    run(api, one, &host, "broken_result", "{\"text\":\"x\",\"limit\":1}", 1, "incorrect record type");
    run(api, one, &host, "fail", "{\"text\":\"x\",\"limit\":1}", 2, "intentional Python exception");
    run(api, one, &host, "capture_host", "{\"text\":\"hello\",\"limit\":1}", 0, "saved");
    run(api, one, &host, "expired_host", "{\"text\":\"hello\",\"limit\":1}", 3, "expired");

    PhxHost badhost = host;
    badhost.version = 2;
    PhxReply output = {0};
    assert(api->invoke(one, &badhost, str("query"), str("{}"), &output) == 4);
    assert(!api->create(str("not_existing_phx_module"), str("Search")));
    assert(!api->create(str("demo_search"), str("Nonexistent")));

    api->destroy(one); api->destroy(two);
    assert(dlclose(library) == 0);
    puts("PASS: annotated Python plugin typed methods / host forwarding / negative cases / lifetime");
    return 0;
}
