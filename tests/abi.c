#include "phenix.h"
_Static_assert(sizeof(PhenixHeader)==16,"header");
_Static_assert(sizeof(PhenixBytes)==16,"borrowed bytes");
_Static_assert(sizeof(PhenixReply)==32,"owned reply");
_Static_assert(offsetof(PhenixReply,ptr)==8,"reply pointer");
_Static_assert(sizeof(PhenixStateHostApi)==64,"ABI 3 host");
_Static_assert(sizeof(PhenixStatePluginTable)==64,"ABI 3 plugin");
_Static_assert(offsetof(PhenixStatePluginTable,initialize)==24,"initialize offset");
_Static_assert(offsetof(PhenixStatePluginTable,poll)==48,"poll offset");
int main(void) { return 0; }
