#ifndef PHENIX_POC_ABI_H
#define PHENIX_POC_ABI_H
#include <stdint.h>
#include <stddef.h>
#define PHENIX_POC_VERSION 1u
#define PHENIX_POC_SCHEMA UINT64_C(0x5048454e49585811)
typedef struct { const uint8_t *data; size_t size; } PhxSlice;
typedef struct { uint8_t *data; size_t size; void (*release)(void *); } PhxOwned;
typedef struct { uint32_t status; uint32_t reserved; PhxOwned body; } PhxReply;
/* Host is a call-local, scoped capability; its validity ends on return. */
typedef struct PhxHost {
    uint32_t version;
    uint32_t table_size;
    uint64_t scope;
    void *context;
    uint32_t (*invoke)(void *, uint64_t, PhxSlice, PhxSlice, PhxReply *);
} PhxHost;
typedef struct { uint32_t version; uint32_t table_size; uint64_t schema; } PhxHeader;
typedef struct PhxTable {
    PhxHeader header;
    void *(*create)(PhxSlice module, PhxSlice class_name);
    uint32_t (*invoke)(void *, const PhxHost *, PhxSlice method, PhxSlice request, PhxReply *);
    uint32_t (*describe)(void *, PhxReply *);
    void (*destroy)(void *);
} PhxTable;
typedef const PhxTable *(*PhxEntry)(void);
#endif
