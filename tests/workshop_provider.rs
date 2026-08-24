use del_rs::semantic::provider::{
    CatalogProvider, EventContext, ExternalBinding, ExternalCategory, ExternalPosition,
    ExternalResolution, NameQuery, WorkshopProvider,
};
use del_rs::{FileId, Span};

fn query(namespace: &[&str], name: &str, position: ExternalPosition, arity: usize) -> NameQuery {
    NameQuery {
        namespace: namespace.iter().map(|part| (*part).to_string()).collect(),
        name: name.to_string(),
        position,
        arity,
        span: Span::new(FileId(0), 0, 1),
    }
}

#[test]
fn catalog_provider_preserves_canonical_action_identity_and_metadata() {
    let provider = CatalogProvider::new().expect("built-in catalog");
    let result = provider.resolve(&query(&[], "SmallMessage", ExternalPosition::Value, 2));
    let ExternalResolution::Known(ExternalBinding::Action(action)) = result else {
        panic!("expected catalog-backed action binding");
    };
    assert_eq!(action.canonical_id, "smallMessage");
    let params = action.params.expect("action parameters");
    assert_eq!(params.len(), 2);
    assert_eq!(params[0].name, "VisibleTo");
    assert!(params[0].optional, "VisibleTo has a catalog default");
    assert_eq!(params[0].default.as_deref(), Some("allPlayers"));
    assert_eq!(params[1].name, "Header");
    assert!(!params[1].optional, "Header has no catalog default");
}

#[test]
fn catalog_provider_resolves_direct_value_identity_and_parameters() {
    let provider = CatalogProvider::new().expect("built-in catalog");
    let result = provider.resolve(&query(&[], "Add", ExternalPosition::Value, 2));
    let ExternalResolution::Known(ExternalBinding::Value(value)) = result else {
        panic!("expected catalog-backed value binding");
    };
    assert_eq!(value.canonical_id, "add");
    let params = value.signature.expect("value signature").params;
    assert_eq!(params.len(), 2);
    assert_eq!(params[0].name, "a");
    assert!(!params[0].optional);
    assert_eq!(params[1].name, "b");
    assert!(!params[1].optional);
}

#[test]
fn catalog_provider_resolves_enum_type_through_enum_domain() {
    let provider = CatalogProvider::new().expect("built-in catalog");
    let result = provider.resolve(&query(&[], "Team", ExternalPosition::Type, 0));
    let ExternalResolution::Known(ExternalBinding::Type(ty)) = result else {
        panic!("expected catalog-backed enum type binding");
    };
    assert_eq!(ty.canonical_id, "Team");
    assert_eq!(ty.category, ExternalCategory::EnumLike);
    assert!(ty.constant);
}

#[test]
fn catalog_provider_resolves_del_event_names_to_canonical_ids() {
    let provider = CatalogProvider::new().expect("built-in catalog");
    let result = provider.resolve(&query(
        &["Event"],
        "OngoingPlayer",
        ExternalPosition::Value,
        0,
    ));
    let ExternalResolution::Known(ExternalBinding::Event(event)) = result else {
        panic!("expected catalog-backed event binding");
    };
    assert_eq!(event.canonical_id, "eachPlayer");
    assert_eq!(event.context, Some(EventContext::Player));
}

#[test]
fn catalog_provider_resolves_the_expanded_del_event_inventory() {
    let provider = CatalogProvider::new().expect("built-in catalog");
    for (source, canonical) in [
        ("OnElimination", "playerEarnedElimination"),
        ("OnFinalBlow", "playerDealtFinalBlow"),
        ("OnDamageDealt", "playerDealtDamage"),
        ("OnDamageTaken", "playerTookDamage"),
        ("OnDeath", "playerDied"),
        ("OnHealingDealt", "playerDealtHealing"),
        ("OnHealingTaken", "playerReceivedHealing"),
        ("OnPlayerJoin", "playerJoined"),
        ("OnPlayerLeave", "playerLeft"),
    ] {
        let result = provider.resolve(&query(&["Event"], source, ExternalPosition::Value, 0));
        let ExternalResolution::Known(ExternalBinding::Event(event)) = result else {
            panic!("expected canonical event binding for {source}");
        };
        assert_eq!(event.canonical_id, canonical);
        assert_eq!(event.context, Some(EventContext::Player));
    }
}

#[test]
fn catalog_provider_resolves_enum_member_without_copying_catalog_data() {
    let provider = CatalogProvider::new().expect("built-in catalog");
    let result = provider.resolve(&query(&["Team"], "All", ExternalPosition::Value, 0));
    let ExternalResolution::Known(ExternalBinding::Value(value)) = result else {
        panic!("expected catalog-backed enum binding");
    };
    assert_eq!(value.canonical_id, "Team.ALL");
}

#[test]
fn catalog_provider_does_not_accept_undeclared_enum_spellings() {
    let provider = CatalogProvider::new().expect("built-in catalog");
    let result = provider.resolve(&query(&["Team"], "all", ExternalPosition::Value, 0));
    assert!(matches!(result, ExternalResolution::NotFound));
}

#[test]
fn catalog_provider_maps_del_enum_spelling_to_catalog_identity() {
    let provider = CatalogProvider::new().expect("built-in catalog");
    let result = provider.resolve(&query(
        &["Button"],
        "PrimaryFire",
        ExternalPosition::Value,
        0,
    ));
    let ExternalResolution::Known(ExternalBinding::Value(value)) = result else {
        panic!("expected catalog-backed enum binding");
    };
    assert_eq!(value.canonical_id, "Button.PRIMARY_FIRE");
}

#[test]
fn catalog_provider_rejects_excess_arguments_and_exposes_catalog_identity() {
    let provider = CatalogProvider::new().expect("built-in catalog");
    let result = provider.resolve(&query(&[], "Wait", ExternalPosition::Value, 3));
    assert!(matches!(result, ExternalResolution::DefiniteError(_)));
    assert_eq!(provider.catalog_identity().catalog_version, "0.1.2");
}
