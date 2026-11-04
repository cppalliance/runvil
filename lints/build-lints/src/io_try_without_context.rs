//! `io_try_without_context`: `?` applied directly to a `std::io::Result`.

use clippy_utils::diagnostics::span_lint_and_help;
use clippy_utils::paths::{PathLookup, PathNS};
use clippy_utils::sym;
use rustc_hir::attrs::lang_items::LangItem;
use rustc_hir::{Expr, ExprKind, MatchSource};
use rustc_lint::{LateContext, LateLintPass, LintContext};
use rustc_middle::ty::{self, Ty};
use rustc_session::{declare_lint, declare_lint_pass};

declare_lint! {
    /// ### What it does
    ///
    /// Flags `?` applied directly to a `std::io::Result` when the enclosing
    /// function (or closure, or `try` block) returns a different error type,
    /// so `?` converts the `std::io::Error` with `From`.
    ///
    /// ### Why is this bad?
    ///
    /// A `std::io::Error` names the failure but not the operation or the
    /// path: "No such file or directory (os error 2)". Converted bare by
    /// `?`, it reaches the reader, often a model reading tool output, with
    /// nothing saying which file was missing or what was being done.
    ///
    /// `?` from one `std::io::Result` into another is not flagged: no
    /// conversion happens, and the caller that leaves `std::io::Error`
    /// behind is the one that adds the context.
    ///
    /// ### Example
    ///
    /// ```rust,ignore
    /// fn load(path: &Path) -> Result<String, Error> {
    ///     Ok(std::fs::read_to_string(path)?)
    /// }
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// fn load(path: &Path) -> Result<String, Error> {
    ///     std::fs::read_to_string(path).map_err(|e| {
    ///         Error::new(ErrorCode::ReadFailed, format!("read {}", path.display())).with_cause(&e)
    ///     })
    /// }
    /// ```
    pub IO_TRY_WITHOUT_CONTEXT,
    Deny,
    "`?` converts a `std::io::Error` without adding the operation or path"
}

declare_lint_pass!(IoTryWithoutContext => [IO_TRY_WITHOUT_CONTEXT]);

/// `std::io::Error`, resolved through std's re-export to wherever the
/// toolchain defines it.
static IO_ERROR: PathLookup = PathLookup::new(PathNS::Type, &[sym::std, sym::io, sym::Error]);

impl<'tcx> LateLintPass<'tcx> for IoTryWithoutContext {
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        // `x?` lowers to `match Try::branch(x) { Continue(v) => v,
        // Break(r) => return FromResidual::from_residual(r) }`; the break
        // arm comes first.
        if let ExprKind::Match(scrutinee, arms, MatchSource::TryDesugar(_)) = expr.kind
            && let ExprKind::Call(callee, [operand]) = scrutinee.kind
            && let ExprKind::Path(qpath) = callee.kind
            && cx.tcx.qpath_is_lang_item(qpath, LangItem::TryTraitBranch)
            && !expr.span.in_external_macro(cx.sess().source_map())
            && is_io_result(cx, cx.typeck_results().expr_ty(operand))
            && let Some(arm) = arms.first()
            && let ExprKind::Ret(Some(residual)) | ExprKind::Break(_, Some(residual)) =
                arm.body.kind
            && !is_io_result(cx, cx.typeck_results().expr_ty(residual))
        {
            span_lint_and_help(
                cx,
                IO_TRY_WITHOUT_CONTEXT,
                expr.span,
                "`?` converts a `std::io::Error` that names no operation or path",
                None,
                "convert the error before `?` with a message naming the operation and the path, \
                 e.g. `.map_err(|e| ...)`",
            );
        }
    }
}

/// Whether `ty` is `Result<_, std::io::Error>`.
fn is_io_result<'tcx>(cx: &LateContext<'tcx>, ty: Ty<'tcx>) -> bool {
    if let ty::Adt(adt, args) = ty.kind()
        && cx.tcx.is_diagnostic_item(sym::Result, adt.did())
    {
        IO_ERROR.matches_ty(cx, args.type_at(1))
    } else {
        false
    }
}
