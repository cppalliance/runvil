//! Tests for the model set's read view and the temperature range: the
//! poisoned and healthy view methods, and the finite range check.

use super::*;

/// A model set whose mutex a panicking thread poisoned.
fn poisoned_set() -> Mutex<ModelSet> {
    let set = Mutex::new(ModelSet::default());
    std::thread::scope(|scope| {
        let _ = scope
            .spawn(|| {
                let _guard = set.lock();
                panic!("poison the set");
            })
            .join();
    });
    set
}

#[test]
fn a_poisoned_model_set_reports_the_lock_error_from_every_view_method() {
    let set = poisoned_set();
    assert!(set.lock().is_err(), "a poisoned set cannot be read");
    assert_eq!(ModelView::default_alias(&set), Err(ModelSetError));
    assert_eq!(set.binding("writer"), Err(ModelSetError));
    assert_eq!(ModelSetError.to_string(), "model set mutex was poisoned");
}

#[test]
fn a_healthy_model_set_answers_every_view_method() {
    let set = Mutex::new(ModelSet::default());
    assert!(
        set.lock()
            .expect("a healthy set locks")
            .bindings()
            .is_empty()
    );
    assert_eq!(ModelView::default_alias(&set), Ok(None));
    assert_eq!(set.binding("writer"), Ok(None));
}

#[test]
fn a_model_set_compares_as_a_total_equivalence() {
    fn assert_eq_impl<T: Eq>() {}
    assert_eq_impl::<Temperature>();
    assert_eq_impl::<ModelInvocation>();
    assert_eq_impl::<ModelBinding>();
    assert_eq_impl::<ModelSet>();
}

/// A token count as a context window.
#[track_caller]
fn window(tokens: u32) -> NonZeroU32 {
    NonZeroU32::new(tokens).expect("a fixture window is non-zero")
}

#[test]
fn render_info_returns_the_catalog_info_a_binding_holds_through_with_invocation() {
    let info = ModelInfo::new(
        ModelId::gateway("catalog-model").expect("a fixture model id"),
        "The catalog's description",
        window(65_536),
        ThinkingMode::Switchable,
    )
    .with_provider("acme");
    let binding = ModelBinding::new(
        "writer",
        "The role's description",
        info.id().clone(),
        ModelInvocation::new(),
        window(4096),
    )
    .with_catalog_info(info.clone());
    assert_eq!(binding.render_info(), info);
    let used = binding.with_invocation(ModelInvocation::new().with_thinking(Some(true)));
    assert_eq!(
        used.render_info(),
        info,
        "new request settings keep the catalog info"
    );
}

#[test]
fn a_binding_built_with_new_renders_a_never_thinking_info_from_its_own_parts() {
    let id = ModelId::gateway("raw-model").expect("a fixture model id");
    let bare = ModelBinding::new(
        "raw-model",
        "raw-model",
        id.clone(),
        ModelInvocation::new(),
        window(8192),
    );
    let expected = ModelInfo::new(id, "raw-model", window(8192), ThinkingMode::Never);
    assert_eq!(bare.render_info(), expected);
    assert_eq!(
        bare.with_provider("acme").render_info(),
        expected.with_provider("acme"),
        "the fallback names the binding's provider"
    );
}

#[test]
fn provider_prefers_the_catalog_and_falls_back_to_the_field() {
    let info = ModelInfo::new(
        ModelId::gateway("catalog-model").expect("a fixture model id"),
        "The catalog's description",
        window(65_536),
        ThinkingMode::Switchable,
    )
    .with_provider("acme");
    let from_catalog = ModelBinding::new(
        "writer",
        "The role's description",
        info.id().clone(),
        ModelInvocation::new(),
        window(4096),
    )
    .with_catalog_info(info.clone());
    assert_eq!(
        from_catalog.provider(),
        Some("acme"),
        "a held catalog's provider answers even when with_provider was never called"
    );
    let from_field = ModelBinding::new(
        "writer",
        "The role's description",
        ModelId::gateway("field-model").expect("a fixture model id"),
        ModelInvocation::new(),
        window(4096),
    )
    .with_provider("acme");
    assert_eq!(
        from_field.provider(),
        Some("acme"),
        "a catalog-less binding still reports the provider its setter stored"
    );
    let both = ModelBinding::new(
        "writer",
        "The role's description",
        ModelId::gateway("both-model").expect("a fixture model id"),
        ModelInvocation::new(),
        window(4096),
    )
    .with_provider("field-provider")
    .with_catalog_info(info);
    assert_eq!(
        both.provider(),
        Some("acme"),
        "the catalog outranks the field when a binding holds both, the order the accessor's docs state"
    );
}

#[test]
fn completion_options_reports_the_invocation_it_holds() {
    let set = CompletionOptions::new("analyst")
        .with_temperature(0.2)
        .expect("0.2 is valid")
        .with_thinking(true);
    let invocation = set.invocation();
    assert_eq!(
        invocation.temperature,
        Some(Temperature::new(0.2).expect("0.2 is valid")),
        "the held invocation carries the temperature the setter validated"
    );
    assert_eq!(
        invocation.thinking,
        Some(true),
        "the held invocation carries the thinking switch the setter stored"
    );
    assert_eq!(
        set.temperature(),
        invocation.temperature,
        "the getter and the held invocation cannot disagree on temperature"
    );
    assert_eq!(
        set.thinking(),
        invocation.thinking,
        "the getter and the held invocation cannot disagree on thinking"
    );
}

#[test]
fn with_temperature_rejects_non_finite_and_out_of_range() {
    let base = || CompletionOptions::new("m");
    assert_eq!(
        base().with_temperature(f64::NAN),
        Err(TemperatureError::NotFinite)
    );
    assert_eq!(
        base().with_temperature(f64::INFINITY),
        Err(TemperatureError::NotFinite)
    );
    assert!(matches!(
        base().with_temperature(-0.1),
        Err(TemperatureError::OutOfRange { .. })
    ));
    assert!(matches!(
        base().with_temperature(2.5),
        Err(TemperatureError::OutOfRange { .. })
    ));
    // The range endpoints and an interior value are accepted.
    assert_eq!(
        base()
            .with_temperature(0.0)
            .expect("0.0 is valid")
            .temperature()
            .map(Temperature::get),
        Some(0.0)
    );
    assert_eq!(
        base()
            .with_temperature(TEMPERATURE_MAX)
            .expect("2.0 is valid")
            .temperature()
            .map(Temperature::get),
        Some(TEMPERATURE_MAX)
    );
    assert_eq!(
        base()
            .with_temperature(0.7)
            .expect("0.7 is valid")
            .temperature()
            .map(Temperature::get),
        Some(0.7)
    );
}
