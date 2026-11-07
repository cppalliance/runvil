//! Host services: typed objects the Host hands to the Packs it
//! installs, each under a named id.
//!
//! A Pack names the per-run services its calls read as [`ServiceId`]s,
//! and reads a service through a [`ServiceKey`], which binds an id
//! literal to the provider's Rust type. The Host fills two
//! [`HostServices`] maps: the Host-wide one a Pack's `construct` reads,
//! and each run's own, which its calls read.
//!
//! An id literal is a [`GlobalName`] with exactly one `/`, as in
//! `namespace/name`. [`HostServices::provide`] refuses any other literal.

use std::any::{Any, TypeId};
use std::collections::BTreeMap;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::sync::Arc;

use runvil_types::error::{Error, ErrorCode, FactKey};
use runvil_types::names::GlobalName;
use serde_json::Value;

/// The name of a Host service and the type its provider supplies.
///
/// Ids compare, hash, and display by their `namespace/name` literal
/// alone. Two keys with the same literal therefore name the same service,
/// whatever their types. The type matters only for whether a provider
/// satisfies the id; see [`HostServices::provides`].
#[derive(Clone, Copy)]
pub struct ServiceId {
    /// The `namespace/name` literal.
    literal: &'static str,
    /// The provider's type, as a function so the id builds in a `const`.
    type_id: fn() -> TypeId,
}

impl ServiceId {
    /// The provider type this id names.
    fn provider_type(self) -> TypeId {
        (self.type_id)()
    }
}

impl PartialEq for ServiceId {
    fn eq(&self, other: &ServiceId) -> bool {
        self.literal == other.literal
    }
}

impl Eq for ServiceId {}

impl Hash for ServiceId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.literal.hash(state);
    }
}

impl fmt::Debug for ServiceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ServiceId").field(&self.literal).finish()
    }
}

impl fmt::Display for ServiceId {
    /// Writes the `namespace/name` literal.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.literal)
    }
}

/// Binds the `namespace/name` id of a Host service to the type `T` its
/// provider supplies.
///
/// The crate that defines a service declares its key once, as a `const`.
/// The Host provides the service under that key, and a Pack reads
/// the service through the same key.
pub struct ServiceKey<T: ?Sized + Send + Sync + 'static> {
    id: ServiceId,
    provider: PhantomData<fn() -> Arc<T>>,
}

impl<T: ?Sized + Send + Sync + 'static> ServiceKey<T> {
    /// Builds the key for the service named `literal`.
    ///
    /// Any literal builds a key. `HostServices::provide` checks the literal
    /// when a provider is supplied under this key.
    #[must_use]
    pub const fn new(literal: &'static str) -> ServiceKey<T> {
        ServiceKey {
            id: ServiceId {
                literal,
                type_id: TypeId::of::<T>,
            },
            provider: PhantomData,
        }
    }

    /// Returns the id this key names.
    #[must_use]
    pub const fn id(&self) -> ServiceId {
        self.id
    }
}

impl<T: ?Sized + Send + Sync + 'static> Clone for ServiceKey<T> {
    fn clone(&self) -> ServiceKey<T> {
        *self
    }
}

impl<T: ?Sized + Send + Sync + 'static> Copy for ServiceKey<T> {}

impl<T: ?Sized + Send + Sync + 'static> fmt::Debug for ServiceKey<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ServiceKey").field(&self.id.literal).finish()
    }
}

/// The id literal of the Host-wide service that supplies the Host's
/// runtime as a `tokio::runtime::Handle`.
///
/// A Pack that runs on tokio, and the Host that provides the service,
/// each build `ServiceKey::<Handle>::new(TOKIO_RUNTIME)`. This crate
/// names no tokio type, so nothing here checks the provider's type: a
/// provider supplied as any other type reads as missing, because
/// [`HostServices::get`] returns `None` on a type mismatch.
pub const TOKIO_RUNTIME: &str = "runvil/tokio-runtime";

/// One provider in a [`HostServices`] map, with the type it was supplied
/// as.
#[derive(Clone)]
struct Entry {
    provider_type: TypeId,
    /// An `Arc<T>` for the `T` in `provider_type`.
    provider: Arc<dyn Any + Send + Sync>,
}

/// The services a Host provides, as a map from service id to provider.
///
/// The map records the type each provider was supplied as. Cloning the
/// map shares the providers.
#[derive(Clone, Default)]
pub struct HostServices {
    entries: BTreeMap<&'static str, Entry>,
}

impl HostServices {
    /// Builds an empty map.
    #[must_use]
    pub fn new() -> HostServices {
        HostServices::default()
    }

    /// Adds `provider` to the map under the id of `key`.
    ///
    /// # Errors
    /// Returns an [`InvalidValue`](ErrorCode::InvalidValue) error that names
    /// the literal, with the reason `invalid_id` when the id literal of `key`
    /// fails to parse as a two-segment `namespace/name` id, and
    /// `duplicate_id` when the map already holds a provider under that
    /// literal, whatever the provider's type. The map stays as it was on
    /// either error.
    pub fn provide<T: ?Sized + Send + Sync + 'static>(
        &mut self,
        key: &ServiceKey<T>,
        provider: Arc<T>,
    ) -> Result<(), Error> {
        let literal = key.id.literal;
        if GlobalName::parse(literal).is_err() || literal.matches('/').count() != 1 {
            return Err(refusal(
                "invalid_id",
                format!("service id {literal} is not a namespace/name id"),
                literal,
            ));
        }
        if self.entries.contains_key(literal) {
            return Err(refusal(
                "duplicate_id",
                format!("a service with id {literal} is already provided"),
                literal,
            ));
        }
        self.insert(key, provider);
        Ok(())
    }

    /// Puts `provider` under `key`'s id, replacing any provider already
    /// there. The caller owns the literal's validity.
    fn insert<T: ?Sized + Send + Sync + 'static>(&mut self, key: &ServiceKey<T>, provider: Arc<T>) {
        self.entries.insert(
            key.id.literal,
            Entry {
                provider_type: key.id.provider_type(),
                provider: Arc::new(provider),
            },
        );
    }

    /// Returns the provider supplied under the id of `key`.
    ///
    /// Returns `None` when no provider is under that id, or when the
    /// provider was supplied as a type other than `T`.
    #[must_use]
    pub fn get<T: ?Sized + Send + Sync + 'static>(&self, key: &ServiceKey<T>) -> Option<Arc<T>> {
        self.entries
            .get(key.id.literal)?
            .provider
            .downcast_ref::<Arc<T>>()
            .cloned()
    }

    /// Returns whether the map holds a provider under `id` that was
    /// supplied as the type `id` names.
    #[must_use]
    pub fn provides(&self, id: &ServiceId) -> bool {
        self.entries
            .get(id.literal)
            .is_some_and(|entry| entry.provider_type == id.provider_type())
    }
}

impl fmt::Debug for HostServices {
    /// Lists the provided ids, never the providers.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostServices")
            .field("services", &self.entries.keys().collect::<Vec<_>>())
            .finish()
    }
}

/// The refusal of a provider: an `InvalidValue` that names the id literal,
/// with the broken rule as its reason. Its prose is its message, which names
/// the id and the rule.
fn refusal(reason: &'static str, message: String, literal: &str) -> Error {
    let mut error = Error::new(ErrorCode::InvalidValue, message.clone(), message);
    error.facts.insert(FactKey::Name, Value::from(literal));
    error.facts.insert(FactKey::Reason, Value::from(reason));
    error
}

#[cfg(test)]
mod tests {
    //! Tests for the Host service vocabulary: ids by literal, the typed keys,
    //! and the map's provide, get, and provides rules.

    use std::collections::HashSet;
    use std::sync::Arc;

    use runvil_types::error::{Error, ErrorCode, FactKey};
    use serde_json::Value;

    use super::{HostServices, ServiceId, ServiceKey, TOKIO_RUNTIME};

    /// Asserts `error` refuses the provider under `literal` for `reason`, with
    /// its message as its prose.
    #[track_caller]
    fn assert_refused(error: &Error, reason: &str, literal: &str) {
        assert_eq!(error.code, ErrorCode::InvalidValue, "{error:?}");
        assert_eq!(
            error.facts.get(&FactKey::Reason),
            Some(&Value::from(reason)),
            "{error:?}"
        );
        assert_eq!(
            error.facts.get(&FactKey::Name),
            Some(&Value::from(literal)),
            "{error:?}"
        );
        assert_eq!(error.prose, error.message, "{error:?}");
    }

    /// A text service, built in a `const` as every key is.
    const GREETING: ServiceKey<str> = ServiceKey::new("acme/greeting");

    /// A second key with the greeting's literal and another type.
    const GREETING_AS_NUMBER: ServiceKey<u32> = ServiceKey::new("acme/greeting");

    /// A static slice of ids, the shape `PackInfo::needs` holds.
    const NEEDS: &[ServiceId] = &[GREETING.id()];

    /// Stands in for the runtime handle, which this crate cannot name.
    struct StandInRuntime(u32);

    /// The key a Host provides its runtime under, built from the contract's
    /// name in a `const`.
    const PROVIDED_RUNTIME: ServiceKey<StandInRuntime> = ServiceKey::new(TOKIO_RUNTIME);

    #[test]
    fn a_provided_service_is_found_under_its_key() {
        let mut services = HostServices::new();
        assert!(services.get(&GREETING).is_none());
        assert!(!services.provides(&GREETING.id()));

        services
            .provide(&GREETING, Arc::from("hello"))
            .expect("a valid, new id is accepted");
        assert_eq!(services.get(&GREETING).as_deref(), Some("hello"));
        assert!(services.provides(&GREETING.id()));
        assert!(services.provides(&NEEDS[0]));
    }

    #[test]
    fn a_runtime_provided_under_the_contracts_name_reads_back_through_a_second_key_built_from_it() {
        assert_eq!(PROVIDED_RUNTIME.id().to_string(), "runvil/tokio-runtime");
        let mut services = HostServices::new();
        services
            .provide(&PROVIDED_RUNTIME, Arc::new(StandInRuntime(7)))
            .expect("the runtime service's name is a namespace/name id");

        let read: ServiceKey<StandInRuntime> = ServiceKey::new(TOKIO_RUNTIME);
        assert!(services.provides(&read.id()));
        assert_eq!(services.get(&read).map(|runtime| runtime.0), Some(7));
    }

    #[test]
    fn a_second_provider_under_a_provided_id_is_refused_and_the_first_stays() {
        let mut services = HostServices::new();
        services
            .provide(&GREETING, Arc::from("first"))
            .expect("the first provider is accepted");

        let error = services
            .provide(&GREETING, Arc::from("second"))
            .expect_err("a duplicate id is refused");
        assert_refused(&error, "duplicate_id", "acme/greeting");
        assert_eq!(
            error.to_string(),
            "a service with id acme/greeting is already provided"
        );

        let other_type = services
            .provide(&GREETING_AS_NUMBER, Arc::new(7))
            .expect_err("a duplicate literal is refused whatever its type");
        assert_refused(&other_type, "duplicate_id", "acme/greeting");
        assert_eq!(services.get(&GREETING).as_deref(), Some("first"));
    }

    #[test]
    fn an_id_outside_the_namespace_name_grammar_is_refused() {
        for literal in [
            "greeting",
            "acme/greeting/extra",
            "acme/",
            "Acme/Greeting",
            "acme/greet ing",
        ] {
            let key: ServiceKey<str> = ServiceKey::new(literal);
            let mut services = HostServices::new();
            let error = services
                .provide(&key, Arc::from("hello"))
                .expect_err("an unparseable id is refused");
            assert_refused(&error, "invalid_id", literal);
            assert_eq!(
                error.to_string(),
                format!("service id {literal} is not a namespace/name id")
            );
            assert!(
                std::error::Error::source(&error).is_none(),
                "{literal}: the refusal carries no cause"
            );
            assert!(
                !services.provides(&key.id()),
                "{literal}: nothing is stored"
            );
        }
    }

    #[test]
    fn a_service_id_is_a_global_name_with_exactly_one_slash() {
        for literal in ["acme/greeting/extra", "acme/greet ing", "greeting"] {
            let key: ServiceKey<str> = ServiceKey::new(literal);
            let mut services = HostServices::new();
            let error = services
                .provide(&key, Arc::from("hello"))
                .expect_err("a literal outside the service id grammar is refused");
            assert_refused(&error, "invalid_id", literal);
            assert!(
                std::error::Error::source(&error).is_none(),
                "{literal}: the refusal carries no Pack id parse failure: {error:?}"
            );
            assert!(
                !services.provides(&key.id()),
                "{literal}: nothing is stored"
            );
        }
        let mut services = HostServices::new();
        services
            .provide(
                &ServiceKey::<str>::new("acme.corp/greeting-v2"),
                Arc::from("hello"),
            )
            .expect("a two-segment literal is accepted");
    }

    #[test]
    fn a_provider_under_the_right_id_with_the_wrong_type_is_not_found() {
        let mut services = HostServices::new();
        services
            .provide(&GREETING_AS_NUMBER, Arc::new(7))
            .expect("the number provider is accepted");

        assert!(
            services.get(&GREETING).is_none(),
            "get finds nothing when the stored type differs"
        );
        assert!(
            !services.provides(&GREETING.id()),
            "provides is false when the stored type differs"
        );
        assert_eq!(services.get(&GREETING_AS_NUMBER).as_deref(), Some(&7));
        assert!(services.provides(&GREETING_AS_NUMBER.id()));
    }

    #[test]
    fn ids_compare_hash_and_display_by_their_literal() {
        assert_eq!(GREETING.id(), GREETING_AS_NUMBER.id());
        assert_ne!(GREETING.id(), ServiceKey::<str>::new("acme/farewell").id());
        let ids: HashSet<ServiceId> = [GREETING.id(), GREETING_AS_NUMBER.id()].into();
        assert_eq!(ids.len(), 1, "one literal hashes once");
        assert_eq!(GREETING.id().to_string(), "acme/greeting");
        assert_eq!(
            format!("{:?}", GREETING.id()),
            "ServiceId(\"acme/greeting\")"
        );
    }

    #[test]
    fn a_clone_shares_the_same_providers() {
        let mut services = HostServices::new();
        let provider: Arc<str> = Arc::from("hello");
        services
            .provide(&GREETING, Arc::clone(&provider))
            .expect("the provider is accepted");
        let clone = services.clone();
        let found = clone.get(&GREETING).expect("the clone holds the provider");
        assert!(Arc::ptr_eq(&found, &provider));
    }

    #[test]
    fn debug_lists_the_provided_ids_without_the_providers() {
        let mut services = HostServices::new();
        services
            .provide(&GREETING, Arc::from("secret greeting"))
            .expect("the provider is accepted");
        let shown = format!("{services:?}");
        assert!(shown.contains("acme/greeting"), "{shown}");
        assert!(!shown.contains("secret greeting"), "{shown}");
    }
}
