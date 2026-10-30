/* Intentionally incompatible .so fixture to verify fail-closed loading. */
#include "../include/phenix_native_experiment.h"
#ifndef BAD_KIND
#error "Specify BAD_KIND=1(version),2(size),3(schema)"
#endif
static const PhenixPublicApi bad = {
    .header = {
        .version = BAD_KIND == 1 ? 999 : PHENIX_ABI_VERSION,
        .table_bytes = BAD_KIND == 2 ? 0 : sizeof(PhenixPublicApi),
        .schema = BAD_KIND == 3 ? 1 : PHENIX_SCHEMA_ID,
    },
    .create = 0,
};
__attribute__((visibility("default")))
const PhenixPublicApi *phenix_native_experiment_v1(void) { return &bad; }
