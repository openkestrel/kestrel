//! Compiles the pinned jaq crates through the same API `crates/kestrel-client/src/filter.rs`
//! uses, so the measurement build proves the versions resolve and work.

use jaq_core::data::JustLut;
use jaq_core::load::{Arena, File, Loader};
use jaq_core::{Compiler, Ctx, Vars};
use jaq_json::Val;
use serde::Deserialize as _;
use serde_json::Value;

fn main() {
    let defs = jaq_core::defs()
        .chain(jaq_std::defs())
        .chain(jaq_json::defs());
    let funs = jaq_core::funs::<JustLut<Val>>()
        .chain(jaq_std::funs::<JustLut<Val>>())
        .chain(jaq_json::funs::<JustLut<Val>>());
    let arena = Arena::default();
    let modules = Loader::new(defs)
        .load(
            &arena,
            File {
                code: ".name",
                path: (),
            },
        )
        .expect("the filter should parse");
    let program = Compiler::default()
        .with_funs(funs)
        .compile(modules)
        .expect("the filter should compile");
    let input: Val =
        Val::deserialize(&Value::from(serde_json::json!({ "name": "x" }))).expect("a value");
    let ctx = Ctx::<JustLut<Val>>::new(&program.lut, Vars::new([]));
    let mut yielded = 0usize;
    for output in program.id.run((ctx, input)) {
        let _ = jaq_core::unwrap_valr(output).expect("the filter should run");
        yielded += 1;
    }
    assert_eq!(yielded, 1);
}
