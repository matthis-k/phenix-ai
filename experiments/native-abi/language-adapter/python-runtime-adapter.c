/* One native Python runtime adapter, usable by multiple Python plugin modules.
 * Owns CPython objects; exposes only the unchanged Phenix experiment C table.
 * This *experiment* assumes trusted native code, a single initialization
 * authority, and one process-wide interpreter. It does not implement sandboxing.
 */
#include "../include/phenix_native_experiment.h"
#include <Python.h>
#include <pthread.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
    PyObject *plugin;
} PythonInstance;

static pthread_once_t start_once = PTHREAD_ONCE_INIT;
static int interpreter_ready = 0;

static void start_python(void) {
    Py_Initialize();
    if (Py_IsInitialized()) {
        interpreter_ready = 1;
        PyEval_SaveThread();  /* release initial GIL for calls from any thread */
    }
}

static uint32_t python_invoke(void *context, PhenixRequest request, PhenixResponse *response) {
    if (!context || !response || request.options.mode != 0
        || request.query.len > 4096
        || (request.query.len && !request.query.ptr)) return 1;
    PythonInstance *instance = context;
    PyGILState_STATE gil = PyGILState_Ensure();
    uint32_t status = 2;
    PyObject *query = NULL, *mul = NULL, *max = NULL, *mode = NULL;
    PyObject *method = NULL, *result = NULL;
    query = PyUnicode_DecodeUTF8(
        request.query.len ? (const char *)request.query.ptr : "",
        (Py_ssize_t)request.query.len, "strict"
    );
    if (!query) {
        status = 1;
        goto done;
    }
    mul = PyLong_FromLongLong(request.options.multiplier);
    max = PyLong_FromUnsignedLong(request.options.maximum);
    mode = PyLong_FromUnsignedLong(request.options.mode);
    method = PyObject_GetAttrString(instance->plugin, "invoke");
    if (!mul || !max || !mode || !method || !PyCallable_Check(method)) goto done;
    result = PyObject_CallFunctionObjArgs(method, query, mul, max, mode, NULL);
    if (!result || !PyTuple_Check(result) || PyTuple_Size(result) != 2) goto done;
    PyObject *score_py = PyTuple_GetItem(result, 0);
    PyObject *calls_py = PyTuple_GetItem(result, 1);
    if (!PyLong_Check(score_py) || !PyLong_Check(calls_py)) goto done;
    int64_t score = (int64_t)PyLong_AsLongLong(score_py);
    if (PyErr_Occurred()) goto done;
    uint64_t calls = (uint64_t)PyLong_AsUnsignedLongLong(calls_py);
    if (PyErr_Occurred()) goto done;
    *response = (PhenixResponse) {
        .score = score, .calls = calls, .status = 0, .reserved = 0,
    };
    status = 0;
done:
    if (PyErr_Occurred()) PyErr_Clear();
    Py_XDECREF(result);
    Py_XDECREF(method);
    Py_XDECREF(mode);
    Py_XDECREF(max);
    Py_XDECREF(mul);
    Py_XDECREF(query);
    PyGILState_Release(gil);
    return status;
}

static void python_destroy(void *context) {
    if (!context) return;
    PythonInstance *instance = context;
    PyGILState_STATE gil = PyGILState_Ensure();
    Py_XDECREF(instance->plugin);
    PyGILState_Release(gil);
    free(instance);
}

static PhenixOpaqueObject python_create(void) {
    PhenixOpaqueObject empty = {0};
    if (pthread_once(&start_once, start_python) != 0 || !interpreter_ready) return empty;
    const char *module_name = getenv("PHENIX_PYTHON_PLUGIN_MODULE");
    if (!module_name || !*module_name) module_name = "phenix_plugin_v1";
    PyGILState_STATE gil = PyGILState_Ensure();
    PyObject *module = PyImport_ImportModule(module_name);
    PyObject *klass = module ? PyObject_GetAttrString(module, "Plugin") : NULL;
    PyObject *plugin = klass ? PyObject_CallNoArgs(klass) : NULL;
    Py_XDECREF(klass);
    Py_XDECREF(module);
    if (PyErr_Occurred()) PyErr_Clear();
    PyGILState_Release(gil);
    if (!plugin) return empty;
    PythonInstance *instance = calloc(1, sizeof(*instance));
    if (!instance) {
        gil = PyGILState_Ensure();
        Py_DECREF(plugin);
        PyGILState_Release(gil);
        return empty;
    }
    instance->plugin = plugin;
    return (PhenixOpaqueObject) {
        .context = instance, .invoke = python_invoke, .destroy = python_destroy,
    };
}

static const PhenixPublicApi api = {
    .header = {
        .version = PHENIX_ABI_VERSION,
        .table_bytes = sizeof(PhenixPublicApi),
        .schema = PHENIX_SCHEMA_ID,
    },
    .create = python_create,
};

__attribute__((visibility("default")))
const PhenixPublicApi *phenix_native_experiment_v1(void) { return &api; }
