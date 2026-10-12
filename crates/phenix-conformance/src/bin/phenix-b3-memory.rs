//! Native, SDK-only Skills and Context conformance across process reload.
use phenix_basic_contracts::{
    BasicError, ContextItem, ContextSet, ErrorCode, Scope, Skill, SkillSet,
};
use phenix_contract::{Authority, Portable};
use phenix_kernel::{
    resolve::{Composition, Prepared, ScopedGrant, Selection, SlotBinding},
    Outcome,
};
use std::{collections::BTreeSet, fs, path::Path};

fn selection(key: &str) -> Selection {
    let (interface, operation) = key.split_once('/').unwrap();
    Selection {
        interface: interface.into(),
        operation: operation.into(),
        provider: "phenix.basic.memory".into(),
    }
}
fn invoke<T: Portable>(
    kernel: &phenix_kernel::Kernel,
    key: &str,
    input: impl Portable,
) -> Result<Result<T, BasicError>, phenix_contract::Failure> {
    let root = kernel.admit_root(selection(key))?;
    match root.invoke(&input.to_value())? {
        Outcome::Success(value) => Ok(Ok(T::from_value(value)?)),
        Outcome::Domain(value) => Ok(Err(BasicError::from_value(value)?)),
    }
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 6, "MODE MEMORY STORAGE DEFINITIONS OUTPUT");
    let skill_list = "phenix.basic.skills@1/list";
    let skill_set = "phenix.basic.skills@1/set";
    let context_list = "phenix.basic.context@1/list";
    let context_set = "phenix.basic.context@1/set";
    let storage_read = "phenix.basic.storage@1/read";
    let storage_write = "phenix.basic.storage@1/transact";
    let ops = [skill_list, skill_set, context_list, context_set, storage_read, storage_write];
    let resources = BTreeSet::from(["phenix.session.s1".to_owned()]);
    let grants = [
        ("root", skill_list), ("root", skill_set),
        ("root", context_list), ("root", context_set),
        ("phenix.basic.memory", storage_read),
        ("phenix.basic.memory", storage_write),
    ].into_iter().map(|(caller, key)| {
        let (interface, operation) = key.split_once('/').unwrap();
        ScopedGrant {
            caller: caller.into(),
            interface: interface.into(),
            operation: operation.into(),
            resources: resources.clone(),
        }
    }).collect();
    let policy = Composition {
        format: "phenix.composition.v2".into(),
        artifacts: args[2..4].to_vec(),
        definitions: vec![args[4].clone()],
        entry: selection(skill_list),
        slots: vec![SlotBinding {
            caller: "phenix.basic.memory".into(),
            slot: "Storage".into(),
            index: 0,
            provider: "phenix.basic.storage-sqlite".into(),
            export: "Storage".into(),
        }],
        grants,
        excluded: BTreeSet::new(),
        substitutions: vec![],
        authority: Authority {
            operations: ops.into_iter().map(str::to_owned).collect(),
            resources,
        },
        resource: Some("phenix.session.s1".into()),
    };
    let kernel = Prepared::prepare(policy).unwrap().load().unwrap();
    let session = Scope { session: "s1".into() };
    let skills = vec![Skill {
        id: "rust".into(),
        description: "Rust".into(),
        instructions: "Use Rust APIs".into(),
    }];
    let context = vec![ContextItem {
        id: "repo".into(),
        text: "Phenix is graph-driven".into(),
    }];
    if args[1] == "seed" {
        assert!(invoke::<Vec<Skill>>(&kernel, skill_list, session.clone()).unwrap().unwrap().is_empty());
        assert!(invoke::<Vec<ContextItem>>(&kernel, context_list, session.clone()).unwrap().unwrap().is_empty());
        invoke::<()>(&kernel, skill_set, SkillSet {
            session: "s1".into(), skills: skills.clone()
        }).unwrap().unwrap();
        invoke::<()>(&kernel, context_set, ContextSet {
            session: "s1".into(), items: context.clone()
        }).unwrap().unwrap();
        assert_eq!(invoke::<()>(&kernel, skill_set, SkillSet {
            session: "s2".into(), skills: skills.clone()
        }).unwrap().unwrap_err().code, ErrorCode::Denied);
        assert_eq!(invoke::<()>(&kernel, context_set, ContextSet {
            session: "s1".into(),
            items: vec![context[0].clone(), context[0].clone()],
        }).unwrap().unwrap_err().code, ErrorCode::InvalidInput);
        assert_eq!(invoke::<Vec<Skill>>(&kernel, skill_list, session.clone()).unwrap().unwrap(), skills);
    } else if args[1] == "reload" {
        let saved: Vec<Skill> = invoke(&kernel, skill_list, session.clone()).unwrap().unwrap();
        let saved_context: Vec<ContextItem> = invoke(&kernel, context_list, session.clone()).unwrap().unwrap();
        assert_eq!(saved, skills);
        assert_eq!(saved_context, context);
        invoke::<()>(&kernel, skill_set, SkillSet {
            session: "s1".into(), skills: vec![]
        }).unwrap().unwrap();
        assert!(invoke::<Vec<Skill>>(&kernel, skill_list, session).unwrap().unwrap().is_empty());
        let out = Path::new(&args[5]);
        fs::create_dir_all(out).unwrap();
        fs::write(out.join("memory-restart.json"), serde_json::to_vec_pretty(
            &serde_json::json!({
                "format": "phenix.b3.memory.v1",
                "cases": ["independent-skills-context", "sqlite-restart",
                    "cross-session-denial", "duplicate-id-no-effect",
                    "persistence-of-updates"]
            })
        ).unwrap()).unwrap();
    } else {
        panic!("expected seed or reload");
    }
}
