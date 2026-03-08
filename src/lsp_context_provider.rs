use anyhow::Result;

use crate::cli_args::LspProviderKind;
use crate::lsp_context::{resolve_lsp_context, LspContextLimits, ResolvedLspContext};
use crate::lsp_context_typescript::TypescriptLspContextProvider;
use crate::RunArgs;

pub(crate) fn resolve_default_lsp_context(
    args: &RunArgs,
    limits: LspContextLimits,
) -> Result<Option<ResolvedLspContext>> {
    match args.lsp_provider {
        None => Ok(None),
        Some(LspProviderKind::Typescript) => {
            let provider = TypescriptLspContextProvider::new(args.lsp_command.clone());
            match resolve_lsp_context(&args.workdir, &provider, limits) {
                Ok(resolved) => Ok(resolved),
                Err(_) => Ok(None),
            }
        }
    }
}
