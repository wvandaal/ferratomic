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
fn test_bug_bd_1kh_kani_public_access() {
    // Patterns that indicate direct field access (internal API).
    // Method calls like `store.datoms()` are public API and allowed.
    // Use patterns that exclude the method-call parens.
    let forbidden = [
        "store.indexes.", // field access (vs. store.indexes() method)
        ".datoms.",       // field-chained access (not method call)
        ".tx_epoch",
        "Datom {",
        ".entity.as_bytes()",
        "d.e,",
        "d.a.clone()",
        "d.v.clone()",
    ];

    for (name, source) in kani_sources() {
        for pattern in forbidden {
            assert!(
                !source.contains(pattern),
                "bd-1kh: {name} still depends on internal field pattern `{pattern}`"
            );
        }
    }
}
