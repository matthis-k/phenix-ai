#ifndef PHENIX_NATIVE_V1_H
#define PHENIX_NATIVE_V1_H
#include <stddef.h>
#include <stdint.h>
/* ABI 1.0, Linux x86_64. Status: 0 success, 1 domain, 2 kernel failure.
 * Every reply carries exactly one phenix.value.v1 value. The receiver copies
 * then invokes producer release once. NULL is valid only at zero length.
 * Function pointers are required, including release on every reply. A null
 * release rejects before reading the buffer; no release can then be performed.
 * Non-null pointers must reference valid producer-owned memory/code. Pointer
 * validity cannot be established against hostile in-process native code.
 * Borrowed input and HostApi expire on return.
 * create writes one producer-owned instance; stop destroys it once.
 * Scopes are host-issued and reject use from another thread or after return.
 */
typedef struct { const uint8_t *ptr; size_t len; } PhenixBytes;
typedef struct {
  uint32_t status;
  uint8_t *ptr;
  size_t len;
  void (*release)(uint8_t *, size_t);
} PhenixReply;
typedef struct { uint32_t major, minor, table_bytes, features; } PhenixHeader;
typedef struct {
  uint64_t scope;
  PhenixReply (*invoke)(uint64_t, PhenixBytes, PhenixBytes);
} PhenixHostApi;
typedef struct {
  PhenixHeader header;
  PhenixReply (*create)(void **);
  PhenixReply (*invoke)(void *, const PhenixHostApi *, PhenixBytes, PhenixBytes);
  PhenixReply (*stop)(void *);
} PhenixPluginTable;
/* Validate non-null header pointer, major/minor/features, then table_bytes
 * before reading remaining table entries. Loading trusted code may already
 * execute constructors; offline verification must precede loading. */
const PhenixHeader *phenix_native_v1(void);
#endif

#ifndef PHENIX_NATIVE_V2_H
#define PHENIX_NATIVE_V2_H
/* ABI 2.0, pending feature 1. Wake copies only ticket and permit values.
 * It requests polling from any thread. The HostApi expires on native return.
 * Replies 0/1/5 prove quiescence; 2 does not; 3 is Pending with Null;
 * 4 proves quiescence after cancellation. See m0.2-pending-contracts.md. */
typedef struct {
  PhenixHostApi host;
  uint64_t ticket, permit;
  uint32_t (*wake)(uint64_t, uint64_t);
} PhenixPendingHostApi;
typedef struct {
  PhenixHeader header;
  PhenixReply (*create)(void **);
  PhenixReply (*invoke)(void *, const PhenixPendingHostApi *, PhenixBytes, PhenixBytes);
  PhenixReply (*stop)(void *);
  PhenixReply (*poll)(void *, const PhenixPendingHostApi *, PhenixBytes);
  PhenixReply (*cancel)(void *, const PhenixPendingHostApi *, PhenixBytes);
} PhenixPendingPluginTable;
const PhenixHeader *phenix_native_v2(void);
#endif
