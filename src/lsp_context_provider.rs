use anyhow::Result;

use crate::lsp_context::{
    resolve_lsp_context, DisabledLspContextProvider, LspContextLimits, ResolvedLspContext,
};

pub(crate) fn resolve_default_lsp_context(
    workdir: &std::path::Path,
    limits: LspContextLimits,
) -> Result<Option<ResolvedLspContext>> {
    let provider = DisabledLspContextProvider;
    resolve_lsp_context(workdir, &provider, limits)
}
