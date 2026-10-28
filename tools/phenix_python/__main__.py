"""Generate an independent native crate, without encoding schemas in Python."""
import argparse
import ast
import dataclasses
import importlib.util
import inspect
import json
from pathlib import Path
import re
import sys
import typing

RUST_RESERVED = set("as break const continue crate else enum extern false fn for if impl in let loop match mod move mut pub ref return self Self static struct super trait true type unsafe use where while async await dyn abstract become box do final macro override priv typeof unsized virtual yield try gen".split())


def generate(source, output, sdk):
    source, output, sdk = Path(source).resolve(), Path(output), Path(sdk).resolve()
    spec = importlib.util.spec_from_file_location("_phenix_author", source)
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    try:
        spec.loader.exec_module(module)
        plugins = [v for v in vars(module).values() if inspect.isclass(v)
                   and v.__module__ == module.__name__ and "__phenix_plugin__" in vars(v)]
        if len(plugins) != 1:
            raise ValueError("exactly one @plugin class required")
        cls = plugins[0]
        identity, version, contract = cls.__phenix_plugin__
        for value in (identity, version, contract):
            if not isinstance(value, str) or not re.fullmatch(r"[A-Za-z0-9_.@-]+", value):
                raise ValueError("explicit simple id/version/contract required")
        methods = [(n, v) for n, v in vars(cls).items()
                   if getattr(v, "__phenix_exposed__", False)]
        if len(methods) != 1:
            raise ValueError("exactly one @expose method supported")
        name, method = methods[0]
        if not re.fullmatch(r"[a-z][a-z0-9_]*", name) or name in RUST_RESERVED:
            raise ValueError("method must have a simple lowercase name")
        if (inspect.iscoroutinefunction(method) or inspect.isgeneratorfunction(method)
                or inspect.isasyncgenfunction(method)):
            raise ValueError("async/generator operations unsupported")
        params = list(inspect.signature(method).parameters.values())
        if (len(params) != 2 or params[0].name != "self"
                or any(p.kind != p.POSITIONAL_OR_KEYWORD or p.default != p.empty for p in params)):
            raise ValueError("only (self, request) supported; Host and extra arguments unsupported")
        hints = typing.get_type_hints(method)
        if set(hints) != {params[1].name, "return"}:
            raise ValueError("explicit request and return annotations required; Host unsupported")
        types = {}

        def lower(t):
            if t is str:
                return "String"
            if (not inspect.isclass(t) or not dataclasses.is_dataclass(t)
                    or t.__module__ != module.__name__ or t.__bases__ != (object,)):
                raise ValueError(f"unsupported annotation: {t!r}; use str or flat string dataclass")
            if (not re.fullmatch(r"[A-Z][A-Za-z0-9]*", t.__name__)
                    or t.__name__ in RUST_RESERVED | {"Api", "String", "State", "Result", "Option", "Py", "PyAny", "PyResult", "Python"}):
                raise ValueError("dataclass must have a simple capitalized name")
            fields = dataclasses.fields(t)
            annotations = typing.get_type_hints(t)
            if not fields or set(annotations) != {f.name for f in fields}:
                raise ValueError("empty dataclass or non-field annotations unsupported")
            for f in fields:
                if (annotations[f.name] is not str or not f.init or f.kw_only
                        or not re.fullmatch(r"[a-z][a-z0-9_]*", f.name) or f.name in RUST_RESERVED):
                    raise ValueError("only positional string dataclass fields supported")
            types[t.__name__] = fields
            return t.__name__

        request = lower(hints[params[1].name])
        response = lower(hints["return"])
    finally:
        del sys.modules[spec.name]

    # Remove only authoring syntax; the runtime has no frontend installation dependency.
    tree = ast.parse(source.read_text())
    for node in tree.body:
        if isinstance(node, ast.ImportFrom) and node.module == "phenix_python":
            node.names = []
        if isinstance(node, ast.ClassDef) and node.name == cls.__name__:
            node.decorator_list = [d for d in node.decorator_list
                                   if not (isinstance(d, ast.Call) and isinstance(d.func, ast.Name)
                                           and d.func.id == "plugin")]
            for child in node.body:
                if isinstance(child, (ast.FunctionDef, ast.AsyncFunctionDef)):
                    child.decorator_list = [d for d in child.decorator_list
                                            if not (isinstance(d, ast.Name) and d.id == "expose")]
    tree.body = [n for n in tree.body if not (isinstance(n, ast.ImportFrom) and not n.names)]
    runtime = ast.unparse(tree)
    if "phenix_python" in runtime:
        raise ValueError("use direct 'from phenix_python import plugin, expose' authoring syntax")

    q = json.dumps
    declarations = "\n".join(
        '#[derive(phenix_sdk::Portable)]\npub struct ' + n + ' {\n' +
        ''.join(f'pub {f.name}: String,\n' for f in fields) + '}\n'
        for n, fields in types.items())
    contract_rs = declarations + f'''
#[phenix_sdk::contract(id = {q(contract)})]
pub trait Api {{
    fn {name}(&self, request: {request}) -> Result<{response}, String>;
}}
'''
    if request == "String":
        to_python = 'let argument = request.into_pyobject(py)?.into_any();'
    else:
        args = ''.join(f'request.{f.name},' for f in types[request])
        to_python = f'let argument = _module.getattr({q(request)})?.call1(({args}))?;'
    if response == "String":
        from_python = '''if !result.is_exact_instance_of::<pyo3::types::PyString>() {
            return Err(pyo3::exceptions::PyTypeError::new_err("expected str result"));
        }
        result.extract::<String>()'''
    else:
        fields = ''.join(f'{f.name}: {{ let field = result.getattr({q(f.name)})?;\n'
                         'if !field.is_exact_instance_of::<pyo3::types::PyString>() {\n'
                         'return Err(pyo3::exceptions::PyTypeError::new_err("expected str field")); }\n'
                         'field.extract::<String>()? },\n' for f in types[response])
        from_python = f'''if !result.get_type().is(&_module.getattr({q(response)})?) {{
            return Err(pyo3::exceptions::PyTypeError::new_err("wrong result dataclass"));
        }}
        Ok({response} {{ {fields} }})'''
    lib = f'''
include!("../contract.rs");
use pyo3::prelude::*;
#[used]
#[no_mangle]
#[link_section = ".phenix.graph"]
pub static PHENIX_GRAPH_V2: [u8; phenix_sdk::NATIVE_SECTION_BYTES] =
    *include_bytes!(concat!(env!("OUT_DIR"), "/graph.bin"));

#[phenix_sdk::plugin(id = {q(identity)}, version = {q(version)},
    define(Api), provide(Api, slot = "Api"))]
mod implementation {{
    use super::*;
    #[derive(Default)]
    pub struct State {{ runtime: Option<(Py<pyo3::types::PyModule>, Py<PyAny>)> }}
    #[provide(Api::{name})]
    fn {name}(state: &mut State, request: {request}) -> phenix_sdk::CallResult<{response}, String> {{
        // The kernel's RTLD_LOCAL image must not hide CPython from its extension modules.
        static PYTHON_LIBRARY: std::sync::OnceLock<Result<(), String>> = std::sync::OnceLock::new();
        PYTHON_LIBRARY.get_or_init(|| {{
            // SAFETY: promote the same libpython selected at build/link time; retain it
            // for the process lifetime, as CPython itself is not finalized here.
            let library = unsafe {{ libloading::os::unix::Library::open(
                Some(env!("PHENIX_LIBPYTHON")),
                libloading::os::unix::RTLD_NOW | libloading::os::unix::RTLD_GLOBAL) }};
            library.map(|library| {{ std::mem::forget(library); }}).map_err(|error| error.to_string())
        }}).as_ref().map_err(|error| phenix_sdk::CallError::Kernel(
            phenix_sdk::Failure::new(phenix_sdk::Code::PluginFailed, error.clone())))?;
        Python::with_gil(|py| -> PyResult<{response}> {{
            if state.runtime.is_none() {{
                let id: String = py.import("uuid")?.getattr("uuid4")?.call0()?.getattr("hex")?.extract()?;
                let module_name = format!("_phenix_instance_{{id}}");
                let code = std::ffi::CString::new(include_str!("../runtime.py")).unwrap();
                let module = pyo3::types::PyModule::new(py, &module_name)?;
                let modules = py.import("sys")?.getattr("modules")?.downcast_into::<pyo3::types::PyDict>()?;
                modules.set_item(&module_name, &module)?;
                let initialized = py.run(&code, Some(&module.dict()), None);
                // Never remove a replacement installed while authored code released the GIL.
                if modules.get_item(&module_name)?.is_some_and(|current| current.is(&module)) {{
                    modules.del_item(&module_name)?;
                }}
                initialized?;
                let object = module.getattr({q(cls.__name__)})?.call0()?;
                state.runtime = Some((module.unbind(), object.unbind()));
            }}
            let (module, object) = state.runtime.as_ref().unwrap();
            let _module = module.bind(py);
            {to_python}
            let result = object.bind(py).call_method1({q(name)}, (argument,))?;
            {from_python}
        }}).map_err(|error| phenix_sdk::CallError::Kernel(phenix_sdk::Failure::new(
            phenix_sdk::Code::PluginFailed, format!("Python operation failed: {{error}}"))))
    }}
}}
'''
    build = f'''
include!("contract.rs");
fn main() {{
    use phenix_sdk::{{graph::v2::*, Operation as _, ValueSchema}};
    println!("cargo:rerun-if-changed=contract.rs");
    println!("cargo:rerun-if-env-changed=PYO3_PYTHON");
    let python = std::env::var("PYO3_PYTHON").unwrap_or_else(|_| "python3".into());
    let library = std::process::Command::new(python).args(["-c",
        "import sysconfig, os; print(os.path.join(sysconfig.get_config_var('LIBDIR'), sysconfig.get_config_var('LDLIBRARY')))"])
        .output().expect("Python development interpreter required");
    assert!(library.status.success(), "cannot locate libpython");
    println!("cargo:rustc-env=PHENIX_LIBPYTHON={{}}", String::from_utf8(library.stdout).unwrap().trim());
    let definition = Api::{name}::DEFINITION;
    let request = ValueSchema::decode(definition.request.encode().bytes()).unwrap();
    let security = PortSecurity {{ confidentiality: Classification::Public, integrity: IntegrityClass::Untrusted }};
    let execution = ExecutionPolicy {{ on_domain_error: DomainErrorPolicy::RouteErr }};
    let source = {q(identity + '/source')};
    let node = {q(identity + '/' + name)};
    let graph = GraphContributionV2 {{
        contributor: {q(identity)}.into(), patches: vec![],
        sources: vec![SourceDecl {{ id: source.into(), owner: {q(identity)}.into(),
            kind: SourceKind::ExternalAction {{ action: {q(identity + '.submit')}.into() }},
            payload: request.clone(), security }}],
        nodes: vec![
            NodeDecl {{ id: source.into(), kind: NodeKind::Source {{ source: source.into() }},
                inputs: vec![], outputs: vec![OutputPort {{ name: "out".into(), schema: request.clone(), security }}], execution: execution.clone() }},
            NodeDecl {{ id: node.into(), kind: NodeKind::Invoke {{ selector: String::new(), operation: Api::{name}::KEY.into() }},
                inputs: vec![InputPort {{ name: "in".into(), schema: request, readiness: InputReadiness::One, security }}],
                outputs: vec![OutputPort {{ name: "ok".into(), schema: ValueSchema::decode(definition.response.encode().bytes()).unwrap(), security }},
                    OutputPort {{ name: "err".into(), schema: ValueSchema::decode(definition.error.encode().bytes()).unwrap(), security }}], execution }}],
        edges: vec![EdgeDecl {{ id: {q(identity + '/start')}.into(), kind: EdgeKind::Flow {{
            from: OutPortRef {{ node: source.into(), port: "out".into() }},
            to: InPortRef {{ node: node.into(), port: "in".into() }} }} }}],
    }};
    std::fs::write(std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("graph.bin"), graph.native_section().unwrap()).unwrap();
}}
'''
    manifest = f'''[workspace]
[package]
name = "phenix-python-authored"
version = "0.1.0"
edition = "2021"
[lib]
crate-type = ["cdylib"]
[dependencies]
phenix-sdk = {{ path = {q(str(sdk))} }}
pyo3 = {{ version = "0.25.1", features = ["auto-initialize"] }}
libloading = "0.8"
[build-dependencies]
phenix-sdk = {{ path = {q(str(sdk))} }}
'''
    for path, text in {"Cargo.toml": manifest, "contract.rs": contract_rs,
                       "build.rs": build, "src/lib.rs": lib, "runtime.py": runtime}.items():
        target = output / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text + "\n")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source")
    parser.add_argument("output")
    parser.add_argument("--sdk", required=True, help="public phenix-sdk crate directory")
    args = parser.parse_args()
    generate(args.source, args.output, args.sdk)
