/* Deliberate ABI faults. Nix attaches the macro-produced provider descriptor;
 * these artifacts are negative fixtures, never production plugin packages. */
#include "phenix.h"
#if PHENIX_BADCASE == 1
_Alignas(8) static PhenixHeader bad = {99,0,16,0};
#elif PHENIX_BADCASE == 2
_Alignas(8) static PhenixHeader bad = {1,0,16,0};
#else
static PhenixPluginTable bad = {{1,0,sizeof(PhenixPluginTable),0},NULL,NULL,NULL};
#endif
const PhenixHeader *phenix_native_v1(void) { return (const PhenixHeader *)&bad; }
