//! Read's arguments: what the call asks for, and what is refused before
//! any file is touched.

use runvil_plugin::Error;
use serde_json::Value;

use crate::check::{make_invalid_arguments_error, refuse_blank, refuse_unknown};
use crate::spec::Definition;

/// What a call asks Read for.
#[derive(Debug)]
pub(super) struct Request<'a> {
    /// The path as the model wrote it.
    pub(super) path: &'a str,
    /// The line to start at: `0` and an absent offset read as 1, and a
    /// negative one counts back from the end.
    pub(super) offset: Option<i64>,
    /// The most lines to read, at least 1.
    pub(super) limit: Option<usize>,
}

/// Reads the call's arguments after the shared schema check.
///
/// The order is fixed: an undeclared key, then a blank path, then the
/// offset, then the limit, so a call that breaks two rules always hears
/// about the same one.
///
/// # Errors
/// Returns an invalid-arguments [`Error`] naming the parameter.
pub(super) fn parse<'a>(definition: &Definition, args: &'a Value) -> Result<Request<'a>, Error> {
    let tool = definition.wire_name.as_str();
    refuse_unknown(tool, &definition.parameters, args)?;
    let path = args.get("path").and_then(Value::as_str).unwrap_or_default();
    refuse_blank(tool, "path", path)?;
    let offset = integer(tool, "offset", args)?.map(|(offset, _)| offset);
    let limit = match integer(tool, "limit", args)? {
        Some((limit, given)) if limit < 1 => {
            return Err(make_invalid_arguments_error(
                tool,
                format!("`limit` must be at least 1, and the call passed {given}"),
            ));
        }
        Some((limit, _)) => Some(usize::try_from(limit).unwrap_or(usize::MAX)),
        None => None,
    };
    Ok(Request {
        path,
        offset,
        limit,
    })
}

/// The integer under `name` in `args` with the value as the call wrote
/// it, or `None` when the call left it out or passed `null`.
///
/// The shared check has already refused anything that is not a whole
/// number, so a value that still has no `i64` is a whole number past the
/// 64-bit range, which is refused here.
fn integer<'a>(tool: &str, name: &str, args: &'a Value) -> Result<Option<(i64, &'a Value)>, Error> {
    let Some(value) = args.get(name).filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    whole_number(value)
        .map(|number| Some((number, value)))
        .ok_or_else(|| {
            make_invalid_arguments_error(
                tool,
                format!(
                    "`{name}` must be a JSON integer that fits in 64 bits, \
                     and the call passed {value}"
                ),
            )
        })
}

/// The `i64` a JSON number holds, counting a whole number written with a
/// fraction part such as `3.0`, which the shared check accepts as an
/// integer. `None` for a number outside the `i64` range.
fn whole_number(value: &Value) -> Option<i64> {
    if let Some(integer) = value.as_i64() {
        return Some(integer);
    }
    let float = value.as_f64()?;
    if float.fract() != 0.0 || !(-I64_BOUND..I64_BOUND).contains(&float) {
        return None;
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the value is whole and inside the i64 range, so the cast is exact"
    )]
    Some(float as i64)
}

/// 2^63, the first `f64` above the `i64` range (`-2^63` is `i64::MIN`).
const I64_BOUND: f64 = 9_223_372_036_854_775_808.0;
