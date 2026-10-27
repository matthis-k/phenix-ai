/* Native Python .so hosts annotated Python classes. ABI is an isolated POC,
 * not a replacement for the shipping Phenix ABI-3 dispatcher.
 */
#include "../include/phenix_poc_abi.h"
#include <Python.h>
#include <pthread.h>
#include <stdlib.h>
#include <string.h>

#define MAX_PAYLOAD (1024u * 1024u)
typedef struct { PyObject *object; } PythonPlugin;
typedef struct { const PhxHost *host; int active; } Bridge;

static pthread_once_t once = PTHREAD_ONCE_INIT;
static int initialized = 0;
static void initialize(void) {
    Py_Initialize();
    if (Py_IsInitialized()) {
        initialized = 1;
        PyEval_SaveThread();
    }
}
static void release_body(void *data) { free(data); }
static int fill_reply(PhxReply *reply, uint32_t status, const char *body, size_t len) {
    if (len > MAX_PAYLOAD) return 0;
    uint8_t *buffer = malloc(len ? len : 1);
    if (!buffer) return 0;
    if (len) memcpy(buffer, body, len);
    reply->status = status;
    reply->reserved = 0;
    reply->body = (PhxOwned){buffer, len, release_body};
    return 1;
}
static PyObject *decoded(PhxSlice input) {
    if (input.size > MAX_PAYLOAD || (input.size && !input.data)) {
        PyErr_SetString(PyExc_ValueError, "invalid bounded input");
        return NULL;
    }
    return PyUnicode_DecodeUTF8(input.size ? (const char *)input.data : "", (Py_ssize_t)input.size, "strict");
}
static void bridge_destroy(PyObject *capsule) {
    Bridge *bridge = PyCapsule_GetPointer(capsule, "phenix.host.poc");
    free(bridge);
}
static PyObject *host_call(PyObject *self, PyObject *args) {
    Bridge *bridge = PyCapsule_GetPointer(self, "phenix.host.poc");
    if (!bridge) return NULL;
    if (!bridge->active || !bridge->host) {
        PyErr_SetString(PyExc_PermissionError, "host handle expired");
        return NULL;
    }
    const char *operation;
    PyObject *request;
    if (!PyArg_ParseTuple(args, "sO", &operation, &request)) return NULL;
    PyObject *json = PyImport_ImportModule("json");
    if (!json) return NULL;
    PyObject *encoded = PyObject_CallMethod(json, "dumps", "O", request);
    if (!encoded) { Py_DECREF(json); return NULL; }
    Py_ssize_t length;
    const char *bytes = PyUnicode_AsUTF8AndSize(encoded, &length);
    if (!bytes || length < 0 || (size_t)length > MAX_PAYLOAD) {
        Py_DECREF(encoded); Py_DECREF(json);
        PyErr_SetString(PyExc_ValueError, "invalid host request");
        return NULL;
    }
    const PhxHost *host = bridge->host;
    if (host->version != PHENIX_POC_VERSION || host->table_size != sizeof(PhxHost) || !host->invoke) {
        Py_DECREF(encoded); Py_DECREF(json);
        PyErr_SetString(PyExc_PermissionError, "host ABI unavailable");
        return NULL;
    }
    PhxReply result = {0};
    uint32_t status = host->invoke(host->context, host->scope,
        (PhxSlice){(const uint8_t *)operation, strlen(operation)},
        (PhxSlice){(const uint8_t *)bytes, (size_t)length}, &result);
    Py_DECREF(encoded);
    if (status != 0 || result.status != 0 || !result.body.release || result.body.size > MAX_PAYLOAD
        || (result.body.size && !result.body.data)) {
        if (result.body.release) result.body.release(result.body.data);
        Py_DECREF(json);
        if (status == 3 || result.status == 3)
            PyErr_SetString(PyExc_PermissionError, "kernel rejected selected service call");
        else
            PyErr_SetString(PyExc_RuntimeError, "kernel service failure");
        return NULL;
    }
    PyObject *text = PyUnicode_DecodeUTF8((const char *)result.body.data, (Py_ssize_t)result.body.size, "strict");
    result.body.release(result.body.data);
    if (!text) { Py_DECREF(json); return NULL; }
    PyObject *value = PyObject_CallMethod(json, "loads", "O", text);
    Py_DECREF(text);
    Py_DECREF(json);
    return value;
}
static PyMethodDef callback_def = {"_native_invoke", host_call, METH_VARARGS, "call a selected Phenix service"};

static void *create(PhxSlice module, PhxSlice class_name) {
    if (pthread_once(&once, initialize) != 0 || !initialized) return NULL;
    PyGILState_STATE gil = PyGILState_Ensure();
    PyObject *modname = decoded(module), *classname = decoded(class_name);
    PyObject *mod = modname ? PyImport_Import(modname) : NULL;
    PyObject *klass = (mod && classname) ? PyObject_GetAttr(mod, classname) : NULL;
    PyObject *exports = klass ? PyObject_GetAttrString(klass, "__phenix_exports__") : NULL;
    PyObject *instance = (exports && PyDict_Check(exports) && PyDict_Size(exports) > 0)
        ? PyObject_CallNoArgs(klass) : NULL;
    if (PyErr_Occurred()) PyErr_Clear();
    Py_XDECREF(exports); Py_XDECREF(klass); Py_XDECREF(mod); Py_XDECREF(classname); Py_XDECREF(modname);
    PyGILState_Release(gil);
    if (!instance) return NULL;
    PythonPlugin *plugin = calloc(1, sizeof(*plugin));
    if (!plugin) {
        gil = PyGILState_Ensure(); Py_DECREF(instance); PyGILState_Release(gil);
        return NULL;
    }
    plugin->object = instance;
    return plugin;
}

static uint32_t invoke(void *opaque, const PhxHost *host, PhxSlice method,
                       PhxSlice request, PhxReply *reply) {
    if (!opaque || !reply || !host || !host->invoke || host->version != PHENIX_POC_VERSION
        || host->table_size != sizeof(PhxHost)) return 4;
    *reply = (PhxReply){0};
    PyGILState_STATE gil = PyGILState_Ensure();
    uint32_t status = 4;
    Bridge *bridge = calloc(1, sizeof(*bridge));
    if (!bridge) { PyGILState_Release(gil); return 4; }
    bridge->host = host; bridge->active = 1;
    PyObject *capsule = PyCapsule_New(bridge, "phenix.host.poc", bridge_destroy);
    if (!capsule) { free(bridge); PyErr_Clear(); PyGILState_Release(gil); return 4; }
    PyObject *callback = PyCFunction_NewEx(&callback_def, capsule, NULL);
    PyObject *phenix = PyImport_ImportModule("phenix");
    PyObject *host_class = phenix ? PyObject_GetAttrString(phenix, "Host") : NULL;
    PyObject *host_object = (host_class && callback) ? PyObject_CallOneArg(host_class, callback) : NULL;
    PyObject *dispatch = phenix ? PyObject_GetAttrString(phenix, "dispatch") : NULL;
    PyObject *method_name = decoded(method), *request_text = decoded(request);
    PyObject *result = (dispatch && host_object && method_name && request_text)
        ? PyObject_CallFunctionObjArgs(dispatch, ((PythonPlugin *)opaque)->object,
                                       method_name, request_text, host_object, NULL) : NULL;
    if (result && PyTuple_Check(result) && PyTuple_GET_SIZE(result) == 2) {
        PyObject *s = PyTuple_GET_ITEM(result, 0);
        PyObject *b = PyTuple_GET_ITEM(result, 1);
        if (PyLong_Check(s) && PyUnicode_Check(b)) {
            unsigned long outcome = PyLong_AsUnsignedLong(s);
            Py_ssize_t count;
            const char *body = PyUnicode_AsUTF8AndSize(b, &count);
            if (body && !PyErr_Occurred() && outcome <= 4 && count >= 0
                && fill_reply(reply, (uint32_t)outcome, body, (size_t)count)) status = (uint32_t)outcome;
        }
    }
    // An escaped Python Host retains the callback but can no longer access the host.
    bridge->active = 0;
    bridge->host = NULL;
    if (PyErr_Occurred()) PyErr_Clear();
    Py_XDECREF(result); Py_XDECREF(request_text); Py_XDECREF(method_name);
    Py_XDECREF(dispatch); Py_XDECREF(host_object); Py_XDECREF(host_class);
    Py_XDECREF(phenix); Py_XDECREF(callback); Py_DECREF(capsule);
    PyGILState_Release(gil);
    return status;
}
static uint32_t describe(void *opaque, PhxReply *reply) {
    if (!opaque || !reply) return 4;
    *reply = (PhxReply){0};
    PyGILState_STATE gil = PyGILState_Ensure();
    PyObject *phenix = PyImport_ImportModule("phenix");
    PyObject *manifest = phenix ? PyObject_GetAttrString(phenix, "manifest") : NULL;
    PyObject *type = PyObject_Type(((PythonPlugin *)opaque)->object);
    PyObject *output = (manifest && type) ? PyObject_CallOneArg(manifest, type) : NULL;
    uint32_t status = 4;
    if (output && PyUnicode_Check(output)) {
        Py_ssize_t n;
        const char *data = PyUnicode_AsUTF8AndSize(output, &n);
        if (data && n >= 0 && fill_reply(reply, 0, data, (size_t)n)) status = 0;
    }
    if (PyErr_Occurred()) PyErr_Clear();
    Py_XDECREF(output); Py_XDECREF(type); Py_XDECREF(manifest); Py_XDECREF(phenix);
    PyGILState_Release(gil);
    return status;
}
static void destroy(void *opaque) {
    if (!opaque) return;
    PythonPlugin *plugin = opaque;
    PyGILState_STATE gil = PyGILState_Ensure();
    Py_DECREF(plugin->object);
    PyGILState_Release(gil);
    free(plugin);
}
static const PhxTable table = {
    .header = {PHENIX_POC_VERSION, sizeof(PhxTable), PHENIX_POC_SCHEMA},
    .create = create, .invoke = invoke, .describe = describe, .destroy = destroy,
};
__attribute__((visibility("default"))) const PhxTable *phenix_ergonomic_poc_v1(void) {
    return &table;
}
