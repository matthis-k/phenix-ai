#ifndef PHENIX_NATIVE_V3_H
#define PHENIX_NATIVE_V3_H
#include <stddef.h>
#include <stdint.h>
typedef struct { const uint8_t *ptr; size_t len; } PhenixBytes;
typedef struct {
  uint32_t status;
  uint8_t *ptr;
  size_t len;
  void (*release)(uint8_t *, size_t);
} PhenixReply;
typedef struct { uint32_t major, minor, table_bytes, features; } PhenixHeader;
/* ABI 3.0, features 3. All reply bodies select phenix.value.v2.
 * Validate exact header version/features/length before reading callbacks.
 * Reject null callbacks. Host and borrowed inputs expire on return; only wake,
 * ticket and permit can be copied for later use. Create has no host. Initialize
 * issues cells before admission; effects require authenticated live scopes.
 * See spec/m1-metadata-state-contract.md for requests and reply ownership. */
typedef struct {
  PhenixHeader header;
  uint64_t scope, ticket, permit;
  PhenixReply (*invoke)(uint64_t, PhenixBytes, PhenixBytes, PhenixBytes);
  PhenixReply (*state)(uint64_t, uint32_t, PhenixBytes);
  uint32_t (*wake)(uint64_t, uint64_t);
} PhenixStateHostApi;
typedef struct {
  PhenixHeader header;
  PhenixReply (*create)(void **);
  PhenixReply (*initialize)(void *, const PhenixStateHostApi *);
  PhenixReply (*invoke)(void *, const PhenixStateHostApi *, PhenixBytes, PhenixBytes);
  PhenixReply (*stop)(void *);
  PhenixReply (*poll)(void *, const PhenixStateHostApi *, PhenixBytes);
  PhenixReply (*cancel)(void *, const PhenixStateHostApi *, PhenixBytes);
} PhenixStatePluginTable;
const PhenixHeader *phenix_native_v3(void);
#endif
