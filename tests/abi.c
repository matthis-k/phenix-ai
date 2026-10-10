#include "phenix.h"
_Static_assert(sizeof(PhenixHeader)==16,"header");
_Static_assert(sizeof(PhenixBytes)==16,"borrowed bytes");
_Static_assert(sizeof(PhenixReply)==32,"owned reply");
_Static_assert(offsetof(PhenixReply,ptr)==8,"reply pointer");
_Static_assert(sizeof(PhenixHostApi)==16,"scoped host");
_Static_assert(sizeof(PhenixPluginTable)==40,"plugin table");
int main(void) { return 0; }
