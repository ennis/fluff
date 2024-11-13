//! Dependent expressions.

/*
use std::collections::HashSet;
use syn::visit_mut::VisitMut;

pub(crate) struct DepExpr {
    pub expr: syn::Expr,
    pub deps: Vec<syn::Ident>,
}

pub(crate) struct DepFinder {
    in_scope: HashSet<syn::Ident>,
    deps: Vec<syn::Ident>,
}

impl<'ast> VisitMut<'ast> for DepFinder {
    fn visit_ident_mut(&mut self, i: &mut syn::Ident) {
        self.deps.push(i.clone());
    }
}*/