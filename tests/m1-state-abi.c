#include "phenix.h"
#include <stdlib.h>
#include <string.h>
_Static_assert(sizeof(PhenixStateHostApi)==64,"state host size");
_Static_assert(offsetof(PhenixStateHostApi,invoke)==40,"invoke offset");
_Static_assert(offsetof(PhenixStateHostApi,state)==48,"state offset");
_Static_assert(offsetof(PhenixStateHostApi,wake)==56,"wake offset");
_Static_assert(sizeof(PhenixStatePluginTable)==64,"state plugin size");
_Static_assert(offsetof(PhenixStatePluginTable,initialize)==24,"initialize offset");
_Static_assert(offsetof(PhenixStatePluginTable,cancel)==56,"cancel offset");
static size_t releases;
static void release_reply(uint8_t *ptr,size_t len) { (void)len; ++releases; free(ptr); }
static PhenixReply reply(uint32_t status,const uint8_t *data,size_t len) {
  uint8_t *ptr=malloc(len); if (!ptr) abort(); memcpy(ptr,data,len);
  return (PhenixReply){status,ptr,len,release_reply};
}
static const uint8_t null_v2[]={0x50,0x56,2,0,0};
static const uint8_t create_request[]={0x50,0x56,2,0,7,0,0,0,2,5,0,0,0,7,'c','o','u','n','t','e','r',4,0,0,0,0,0,0,0,0};
typedef struct { int initialized; uint8_t location[117]; } Instance;
static int host_valid(const PhenixStateHostApi *host) {
  return host && host->header.major==3 && host->header.minor==0 && host->header.features==3 && host->header.table_bytes==sizeof(*host) && host->scope && host->invoke && host->state && host->wake;
}
static PhenixReply create(void **out) {
  *out=calloc(1,sizeof(Instance)); if (!*out) abort();
  return reply(0,null_v2,sizeof(null_v2));
}
static PhenixReply initialize(void *raw,const PhenixStateHostApi *host) {
  Instance *instance=raw;
  if (!host_valid(host) || instance->initialized) abort();
  PhenixReply result=host->state(host->scope,1,(PhenixBytes){create_request,sizeof(create_request)});
  if (result.status || result.len!=117 || !result.release || memcmp(result.ptr,"PV\2\0\11",5)) abort();
  memcpy(instance->location,result.ptr,117); result.release(result.ptr,result.len);
  instance->initialized=1;
  return reply(0,null_v2,sizeof(null_v2));
}
static PhenixReply invoke(void *raw,const PhenixStateHostApi *host,PhenixBytes op,PhenixBytes request) {
  (void)op; (void)request; Instance *instance=raw;
  if (!host_valid(host) || !instance->initialized) abort();
  PhenixReply read=host->state(host->scope,2,(PhenixBytes){instance->location,117});
  if (read.status || !read.release) abort();
  read.release(read.ptr,read.len);
  const uint8_t slot[]="counter", operation[]="read";
  PhenixReply called=host->invoke(host->scope,(PhenixBytes){slot,7},(PhenixBytes){operation,4},(PhenixBytes){null_v2,5});
  if (called.status || !called.release) abort();
  called.release(called.ptr,called.len);
  if (host->wake(host->ticket,host->permit)!=0) abort();
  return reply(0,instance->location,117);
}
static PhenixReply stop(void *instance) { free(instance); return reply(0,null_v2,5); }
static PhenixReply poll(void *i,const PhenixStateHostApi *h,PhenixBytes t) { (void)i;(void)h;(void)t;return reply(3,null_v2,5); }
static PhenixReply cancel(void *i,const PhenixStateHostApi *h,PhenixBytes t) { (void)i;(void)h;(void)t;return reply(4,null_v2,5); }
static const PhenixStatePluginTable table={{3,0,64,3},create,initialize,invoke,stop,poll,cancel};
const PhenixHeader *phenix_native_v3(void) { return &table.header; }
size_t m1_fixture_releases(void) { return releases; }
PhenixReply m1_fixture_bad_reply(uint32_t kind) {
  uint8_t bad[]={0x50,0x56,2,0,255};
  if (kind==0) return reply(0,bad,5);
  if (kind==1) return reply(99,null_v2,5);
  if (kind==2) return reply(0,null_v2+4,1);
  if (kind==3) return (PhenixReply){0,NULL,1,NULL};
  const uint8_t reserved[]={0x50,0x56,2,1,0};
  return reply(0,reserved,5);
}
