use wasmtime::Store;

wasmtime::component::bindgen!(in "tests/runtime/resource_into_inner");

use exports::test::resource_into_inner::test::Test;

#[test]
fn run() -> anyhow::Result<()> {
    crate::run_test(
        "resource_into_inner",
        |_| Ok(()),
        |store, component, linker| {
            let (u, e) = ResourceIntoInner::instantiate(store, component, linker)?;
            Ok((u.interface0, e))
        },
        run_test,
    )
}

fn run_test(instance: Test, store: &mut Store<crate::Wasi<()>>) -> anyhow::Result<()> {
    instance.call_test(&mut *store)
}
