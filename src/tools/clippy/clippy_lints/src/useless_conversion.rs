use crate::utils::{
    is_type_diagnostic_item, match_def_path, match_trait_method, paths, snippet, snippet_with_macro_callsite,
    span_lint_and_help, span_lint_and_sugg,
};
use if_chain::if_chain;
use rustc_errors::Applicability;
use rustc_hir::{Expr, ExprKind, HirId, MatchSource};
use rustc_lint::{LateContext, LateLintPass};
use rustc_middle::ty::{self, TyS};
use rustc_session::{declare_tool_lint, impl_lint_pass};

declare_clippy_lint! {
    /// **What it does:** Checks for `Into`, `TryInto`, `From`, `TryFrom`,`IntoIter` calls
    /// that useless converts to the same type as caller.
    ///
    /// **Why is this bad?** Redundant code.
    ///
    /// **Known problems:** None.
    ///
    /// **Example:**
    ///
    /// ```rust
    /// // Bad
    /// // format!() returns a `String`
    /// let s: String = format!("hello").into();
    ///
    /// // Good
    /// let s: String = format!("hello");
    /// ```
    pub USELESS_CONVERSION,
    complexity,
    "calls to `Into`, `TryInto`, `From`, `TryFrom`, `IntoIter` that performs useless conversions to the same type"
}

#[derive(Default)]
pub struct UselessConversion {
    try_desugar_arm: Vec<HirId>,
}

impl_lint_pass!(UselessConversion => [USELESS_CONVERSION]);

#[allow(clippy::too_many_lines)]
impl<'a, 'tcx> LateLintPass<'a, 'tcx> for UselessConversion {
    fn check_expr(&mut self, cx: &LateContext<'a, 'tcx>, e: &'tcx Expr<'_>) {
        if e.span.from_expansion() {
            return;
        }

        if Some(&e.hir_id) == self.try_desugar_arm.last() {
            return;
        }

        match e.kind {
            ExprKind::Match(_, ref arms, MatchSource::TryDesugar) => {
                let e = match arms[0].body.kind {
                    ExprKind::Ret(Some(ref e)) | ExprKind::Break(_, Some(ref e)) => e,
                    _ => return,
                };
                if let ExprKind::Call(_, ref args) = e.kind {
                    self.try_desugar_arm.push(args[0].hir_id);
                }
            },

            ExprKind::MethodCall(ref name, .., ref args, _) => {
                if match_trait_method(cx, e, &paths::INTO) && &*name.ident.as_str() == "into" {
                    let a = cx.tables.expr_ty(e);
                    let b = cx.tables.expr_ty(&args[0]);
                    if TyS::same_type(a, b) {
                        let sugg = snippet_with_macro_callsite(cx, args[0].span, "<expr>").to_string();
                        span_lint_and_sugg(
                            cx,
                            USELESS_CONVERSION,
                            e.span,
                            "useless conversion to the same type",
                            "consider removing `.into()`",
                            sugg,
                            Applicability::MachineApplicable, // snippet
                        );
                    }
                }
                if match_trait_method(cx, e, &paths::INTO_ITERATOR) && &*name.ident.as_str() == "into_iter" {
                    let a = cx.tables.expr_ty(e);
                    let b = cx.tables.expr_ty(&args[0]);
                    if TyS::same_type(a, b) {
                        let sugg = snippet(cx, args[0].span, "<expr>").into_owned();
                        span_lint_and_sugg(
                            cx,
                            USELESS_CONVERSION,
                            e.span,
                            "useless conversion to the same type",
                            "consider removing `.into_iter()`",
                            sugg,
                            Applicability::MachineApplicable, // snippet
                        );
                    }
                }
                if match_trait_method(cx, e, &paths::TRY_INTO_TRAIT) && &*name.ident.as_str() == "try_into" {
                    if_chain! {
                        let a = cx.tables.expr_ty(e);
                        let b = cx.tables.expr_ty(&args[0]);
                        if is_type_diagnostic_item(cx, a, sym!(result_type));
                        if let ty::Adt(_, substs) = a.kind;
                        if let Some(a_type) = substs.types().next();
                        if TyS::same_type(a_type, b);

                        then {
                            span_lint_and_help(
                                cx,
                                USELESS_CONVERSION,
                                e.span,
                                "useless conversion to the same type",
                                None,
                                "consider removing `.try_into()`",
                            );
                        }
                    }
                }
            },

            ExprKind::Call(ref path, ref args) => {
                if_chain! {
                    if args.len() == 1;
                    if let ExprKind::Path(ref qpath) = path.kind;
                    if let Some(def_id) = cx.tables.qpath_res(qpath, path.hir_id).opt_def_id();
                    let a = cx.tables.expr_ty(e);
                    let b = cx.tables.expr_ty(&args[0]);

                    then {
                        if_chain! {
                            if match_def_path(cx, def_id, &paths::TRY_FROM);
                            if is_type_diagnostic_item(cx, a, sym!(result_type));
                            if let ty::Adt(_, substs) = a.kind;
                            if let Some(a_type) = substs.types().next();
                            if TyS::same_type(a_type, b);

                            then {
                                let hint = format!("consider removing `{}()`", snippet(cx, path.span, "TryFrom::try_from"));
                                span_lint_and_help(
                                    cx,
                                    USELESS_CONVERSION,
                                    e.span,
                                    "useless conversion to the same type",
                                    None,
                                    &hint,
                                );
                            }
                        }

                        if_chain! {
                            if match_def_path(cx, def_id, &paths::FROM_FROM);
                            if TyS::same_type(a, b);

                            then {
                                let sugg = snippet(cx, args[0].span.source_callsite(), "<expr>").into_owned();
                                let sugg_msg =
                                    format!("consider removing `{}()`", snippet(cx, path.span, "From::from"));
                                span_lint_and_sugg(
                                    cx,
                                    USELESS_CONVERSION,
                                    e.span,
                                    "useless conversion to the same type",
                                    &sugg_msg,
                                    sugg,
                                    Applicability::MachineApplicable, // snippet
                                );
                            }
                        }
                    }
                }
            },

            _ => {},
        }
    }

    fn check_expr_post(&mut self, _: &LateContext<'a, 'tcx>, e: &'tcx Expr<'_>) {
        if Some(&e.hir_id) == self.try_desugar_arm.last() {
            self.try_desugar_arm.pop();
        }
    }
}
