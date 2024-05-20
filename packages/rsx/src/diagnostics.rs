use proc_macro2_diagnostics::Diagnostic;
use quote::ToTokens;

#[derive(Debug, Clone)]
pub struct Diagnostics {
    pub diagnostics: Vec<Diagnostic>,
}

impl Diagnostics {
    pub fn new() -> Self {
        Self {
            diagnostics: vec![],
        }
    }

    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    pub fn extend(&mut self, diagnostics: Vec<Diagnostic>) {
        self.diagnostics.extend(diagnostics);
    }

    pub fn is_empty(&self) -> bool {
        self.diagnostics.is_empty()
    }

    pub fn into_diagnostics(self) -> Vec<Diagnostic> {
        self.diagnostics
    }
}

impl PartialEq for Diagnostics {
    fn eq(&self, other: &Self) -> bool {
        true
    }
}

impl Eq for Diagnostics {}

impl std::hash::Hash for Diagnostics {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {}
}

impl ToTokens for Diagnostics {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        for diagnostic in &self.diagnostics {
            tokens.extend(diagnostic.clone().emit_as_expr_tokens());
        }
    }
}
