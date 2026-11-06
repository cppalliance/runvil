//! The field shape a store failure's error value carries: its `reason`
//! tag and the [`VfsError`](runvil_vfs::VfsError) variant's fields, and the
//! universal error the failure becomes where it leaves the VFS.

use std::collections::BTreeMap;

use runvil_types::error::{Error, ErrorCode, FactKey};
use serde_json::Value;

use super::ErrorField;

/// Each store field's name in a Lua error table beside the fact that holds
/// it in a universal error, in the order a table lists them.
const STORE_FACTS: [(&str, FactKey); 5] = [
    ("path", FactKey::Path),
    ("valid_up_to", FactKey::ValidUpTo),
    ("rule", FactKey::Rule),
    ("anchor", FactKey::Anchor),
    ("count", FactKey::Count),
];

/// Returns the variant's own fields, without `reason`, in the shape
/// `store_error` documents.
fn store_error_fields(error: &runvil_vfs::VfsError) -> Vec<(String, ErrorField)> {
    use runvil_vfs::VfsError;
    let integer = |value: usize| ErrorField::Integer(i64::try_from(value).unwrap_or(i64::MAX));
    match error {
        VfsError::NotFound { path }
        | VfsError::AlreadyExists { path }
        | VfsError::NotADirectory { path }
        | VfsError::IsADirectory { path }
        | VfsError::DirectoryNotEmpty { path }
        | VfsError::InvalidRange { path, .. }
        | VfsError::PermissionDenied { path, .. }
        | VfsError::ApprovalRequired { path, .. }
        | VfsError::ScopeEnded { path }
        | VfsError::ReadOnly { path }
        | VfsError::OutsideRoot { path, .. }
        | VfsError::RootProtected { path }
        | VfsError::CrossMount { path, .. }
        | VfsError::Io { path, .. }
        | VfsError::Unsupported { path, .. }
        | VfsError::Conflict { path, .. } => {
            vec![("path".to_owned(), ErrorField::String(path.clone()))]
        }
        VfsError::NotUtf8 { path, valid_up_to } => vec![
            ("path".to_owned(), ErrorField::String(path.clone())),
            ("valid_up_to".to_owned(), integer(*valid_up_to)),
        ],
        VfsError::InvalidPath { path, reason } => vec![
            ("path".to_owned(), ErrorField::String(path.clone())),
            (
                "rule".to_owned(),
                ErrorField::String(reason.tag().to_owned()),
            ),
        ],
        VfsError::Anchor {
            path,
            anchor,
            count,
        } => vec![
            ("path".to_owned(), ErrorField::String(path.clone())),
            ("anchor".to_owned(), ErrorField::String(anchor.clone())),
            ("count".to_owned(), integer(*count)),
        ],
        // `NoStore` and `Backend` name no path, and `VfsError` is
        // `#[non_exhaustive]`: a variant added later reports no fields
        // until it is named here.
        _ => Vec::new(),
    }
}

/// Returns the full field set a store error value carries, `reason` first,
/// in the shape [`store_error`] documents.
#[cfg(any(test, feature = "test-support"))]
#[must_use]
pub fn store_error_value_fields(error: &runvil_vfs::VfsError) -> Vec<(String, ErrorField)> {
    let mut fields = vec![(
        "reason".to_owned(),
        ErrorField::String(error.tag().to_owned()),
    )];
    fields.extend(store_error_fields(error));
    fields
}

/// Returns the universal error a store failure becomes where it leaves the
/// VFS: the code named after the variant, the variant's fields as facts under
/// the names a Lua error table gives them, the failure's display text as the
/// message, and the code's default prose.
///
/// A store error value carries the `reason` tag, as `VfsError::tag` names
/// it, first, then the variant's own fields: the paths the failure names,
/// plus `anchor` and its integer `count` for an anchor error, `rule` for an
/// invalid path, or the integer `valid_up_to` for text that is not UTF-8.
/// The rule is the tag `PathReason::tag` returns.
///
/// Both store paths, the direct closures a Pack prelude's install calls
/// and the executor's effect path, build their failure here, so a store
/// error value has the same `kind`, `code`, `reason`, and fields on both;
/// only the direct closures put the store's message ahead of the default
/// prose.
#[must_use]
pub fn store_error(error: &runvil_vfs::VfsError) -> Error {
    let code = store_error_code(error);
    let mut converted = Error::new(code, error.to_string(), code.prose());
    for (name, field) in store_error_fields(error) {
        let Some(&(_, key)) = STORE_FACTS.iter().find(|(known, _)| *known == name) else {
            continue;
        };
        let value = match field {
            ErrorField::String(text) => Value::from(text),
            ErrorField::Integer(number) => Value::from(number),
        };
        converted.facts.insert(key, value);
    }
    converted
}

/// Returns the fields a store failure's universal error renders into its
/// Lua error table, the inverse of [`store_error`]: the code's tag as
/// `reason`, then each store fact under the field name
/// [`store_error`] documents for it, a string fact as a string and a
/// count or an offset as an integer.
pub(super) fn store_fact_fields(error: &Error) -> Vec<(String, ErrorField)> {
    let mut fields = vec![(
        "reason".to_owned(),
        ErrorField::String(error.code.tag().to_owned()),
    )];
    for (name, key) in STORE_FACTS {
        let field = match error.facts.get(&key) {
            Some(Value::String(text)) => ErrorField::String(text.clone()),
            Some(value) => match value.as_i64() {
                Some(number) => ErrorField::Integer(number),
                None => continue,
            },
            None => continue,
        };
        fields.push((name.to_owned(), field));
    }
    fields
}

/// Returns the store facts a store failure's Lua error table `fields`
/// hold, the inverse of [`store_fact_fields`]: each store field under its
/// fact, a string field as a string and an integer field as a number.
/// `reason` is the code's tag, not a fact.
pub(super) fn store_field_facts(fields: &BTreeMap<String, ErrorField>) -> BTreeMap<FactKey, Value> {
    STORE_FACTS
        .iter()
        .filter_map(|&(name, key)| {
            let value = match fields.get(name)? {
                ErrorField::String(text) => Value::from(text.as_str()),
                ErrorField::Integer(number) => Value::from(*number),
            };
            Some((key, value))
        })
        .collect()
}

/// The code named after the variant.
fn store_error_code(error: &runvil_vfs::VfsError) -> ErrorCode {
    use runvil_vfs::VfsError;
    match error {
        VfsError::NotFound { .. } => ErrorCode::NotFound,
        VfsError::AlreadyExists { .. } => ErrorCode::AlreadyExists,
        VfsError::NotADirectory { .. } => ErrorCode::NotADirectory,
        VfsError::IsADirectory { .. } => ErrorCode::IsADirectory,
        VfsError::DirectoryNotEmpty { .. } => ErrorCode::DirectoryNotEmpty,
        VfsError::NotUtf8 { .. } => ErrorCode::NotUtf8,
        VfsError::InvalidPath { .. } => ErrorCode::InvalidPath,
        VfsError::InvalidRange { .. } => ErrorCode::InvalidRange,
        VfsError::Anchor { .. } => ErrorCode::Anchor,
        VfsError::PermissionDenied { .. } => ErrorCode::PermissionDenied,
        VfsError::ApprovalRequired { .. } => ErrorCode::ApprovalRequired,
        VfsError::ScopeEnded { .. } => ErrorCode::ScopeEnded,
        VfsError::ReadOnly { .. } => ErrorCode::ReadOnly,
        VfsError::OutsideRoot { .. } => ErrorCode::OutsideRoot,
        VfsError::RootProtected { .. } => ErrorCode::RootProtected,
        VfsError::NoStore => ErrorCode::NoStore,
        VfsError::CrossMount { .. } => ErrorCode::CrossMount,
        VfsError::Io { .. } => ErrorCode::Io,
        VfsError::Unsupported { .. } => ErrorCode::Unsupported,
        VfsError::Conflict { .. } => ErrorCode::Conflict,
        // `VfsError` is `#[non_exhaustive]`: a variant added later is a
        // backend failure until it is named here.
        _ => ErrorCode::Backend,
    }
}
