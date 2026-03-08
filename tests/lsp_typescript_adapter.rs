use std::fs;
use std::path::PathBuf;

use localagent::lsp_context::{resolve_lsp_context, LspContextLimits};
use localagent::lsp_context_typescript::TypescriptLspContextProvider;
use tempfile::tempdir;

#[test]
fn stub_typescript_server_produces_bounded_diagnostics_and_symbol_context() {
    let tmp = tempdir().expect("tmp");
    fs::create_dir_all(tmp.path().join("src")).expect("mkdir src");
    fs::write(
        tmp.path().join("src").join("index.ts"),
        "const value: number = \"oops\";\n",
    )
    .expect("write index.ts");

    let stub = PathBuf::from(env!("CARGO_BIN_EXE_lsp_stub"));
    let provider = TypescriptLspContextProvider::new(Some(stub));
    let ctx = resolve_lsp_context(tmp.path(), &provider, LspContextLimits::default())
        .expect("resolve")
        .expect("context");
    let snapshot = ctx.diagnostics_snapshot.expect("snapshot");
    let symbols = ctx.symbol_context.expect("symbol context");

    assert_eq!(ctx.provider, "typescript_language_server");
    assert_eq!(snapshot.language.as_deref(), Some("typescript"));
    assert_eq!(snapshot.included_count, 1);
    assert_eq!(snapshot.items[0].code, "2322");
    assert_eq!(symbols.query, "value");
    assert_eq!(symbols.symbol_count_total, 1);
    assert_eq!(symbols.definition_count_total, 1);
    assert_eq!(symbols.reference_count_total, 2);
    assert_eq!(symbols.symbols[0].label, "value");
    assert_eq!(
        symbols.definitions[0].path.file_name().and_then(|s| s.to_str()),
        Some("index.ts")
    );
}
