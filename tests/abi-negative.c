/* Deliberate ABI faults. Nix attaches the macro-produced provider descriptor;
 * these artifacts are negative fixtures, never production plugin packages. */
#include "phenix.h"
#include <stdio.h>
#include <stdlib.h>
#if PHENIX_BADCASE == 1
_Alignas(8) static PhenixHeader bad = {99,0,16,0};
#elif PHENIX_BADCASE == 2
_Alignas(8) static PhenixHeader bad = {1,0,16,0};
#elif PHENIX_BADCASE == 3
static PhenixPluginTable bad = {{1,0,sizeof(PhenixPluginTable),0},NULL,NULL,NULL};
#else
static void event(const char *name) {
  const char *path = getenv("PHENIX_LIFECYCLE_WITNESS");
  if (path) { FILE *f = fopen(path, "a"); if (!f) abort(); fprintf(f, "%s\n", name); fclose(f); }
}
static void release(uint8_t *ptr, size_t len) {
  (void)len;
  event("foreign:release");
  free(ptr);
}
static PhenixReply reply(uint32_t status) {
  uint8_t *bytes = malloc(1);
  if (!bytes) abort();
  bytes[0] = 0; /* phenix.value.v1 Null */
  return (PhenixReply){status, bytes, 1, release};
}
static PhenixReply create(void **out) {
  event("foreign:create");
#if PHENIX_BADCASE == 4
  (void)out;
  /* Both invalid fields: callback validation must precede payload access. */
  return (PhenixReply){0, NULL, 1, NULL};
#else
  *out = malloc(1);
  if (!*out) abort();
  return reply(99); /* acquired instance and buffer, recoverable ABI failure */
#endif
}
static PhenixReply invoke(void *instance, const PhenixHostApi *host, PhenixBytes key, PhenixBytes input) {
  (void)instance; (void)host; (void)key; (void)input;
  event("foreign:invoke");
  return reply(99);
}
static PhenixReply stop(void *instance) {
  event("foreign:stop");
  free(instance);
  return reply(0);
}
static PhenixPluginTable bad = {{1,0,sizeof(PhenixPluginTable),0},create,invoke,stop};
#endif
const PhenixHeader *phenix_native_v1(void) { return (const PhenixHeader *)&bad; }
