#ifndef PHENIX_NATIVE_EXPERIMENT_H
#define PHENIX_NATIVE_EXPERIMENT_H
#include <stddef.h>
#include <stdint.h>
#define PHENIX_ABI_VERSION 1
#define PHENIX_SCHEMA_ID UINT64_C(0x5048454e49580001)
typedef struct { const uint8_t *ptr; size_t len; } PhenixByteSlice;
typedef struct { int64_t multiplier; uint32_t maximum; uint32_t mode; } PhenixOptions;
typedef struct { PhenixByteSlice query; PhenixOptions options; } PhenixRequest;
typedef struct { int64_t score; uint64_t calls; uint32_t status; uint32_t reserved; } PhenixResponse;
typedef struct { uint32_t version; uint32_t table_bytes; uint64_t schema; } PhenixApiHeader;
typedef struct {
    void *context;
    uint32_t (*invoke)(void *, PhenixRequest, PhenixResponse *);
    void (*destroy)(void *);
} PhenixOpaqueObject;
typedef struct { PhenixApiHeader header; PhenixOpaqueObject (*create)(void); } PhenixPublicApi;
typedef const PhenixPublicApi *(*PhenixEntry)(void);
#endif
