use wasmtime::Store;

wasmtime::component::bindgen!(in "tests/runtime/resource_borrow_export");

use exports::test::resource_borrow_export::test::Test;

#[test]
fn run() -> anyhow::Result<()> {
    crate::run_test(
        "resource_borrow_export",
        |_| Ok(()),
        |store, component, linker| {
            let (u, e) = ResourceBorrowExport::instantiate(store, component, linker)?;
            Ok((u.interface0, e))
        },
        run_test,
    )
}

fn run_test(instance: Test, store: &mut Store<crate::Wasi<()>>) -> anyhow::Result<()> {
    let thing = instance.thing().call_constructor(&mut *store, 42)?;
    let res = instance.call_foo(&mut *store, thing)?;
    assert_eq!(res, 42 + 1 + 2);
    Ok(())
}
