fn kani_sources() -> [(&'static str, &'static str); 5] {
    [
        ("clock.rs", include_str!("../kani/clock.rs")),
        ("durability.rs", include_str!("../kani/durability.rs")),
        (
            "schema_identity.rs",
            include_str!("../kani/schema_identity.rs"),
        ),
        ("sharding.rs", include_str!("../kani/sharding.rs")),
        ("store_views.rs", include_str!("../kani/store_views.rs")),
    ]
}

#[test]
fn test_bug_bd_8e3_kani_api_surface() {
    for (name, source) in kani_sources() {
        assert!(
            !source.contains("store::{Committed, Store, Transaction}"),
            "bd-8e3: {} still imports Transaction from store module",
            name
        );
        assert!(
            !source.contains("store::{Store, Transaction}"),
            "bd-8e3: {} still mixes Store and Transaction from store module",
            name
        );
        assert!(
            !source.contains("store::Transaction"),
            "bd-8e3: {} still refers to store::Transaction",
            name
        );
    }
}
