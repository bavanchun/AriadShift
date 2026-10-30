use super::*;

#[test]
fn always_sent_nullable_limit_fields_are_required() {
    let document = current();
    for (schema, fields) in [
        ("Quota", &["limit", "remaining"][..]),
        (
            "ConversionLimits",
            &["max_input_bytes", "max_pages", "timeout_s", "max_memory_mb"][..],
        ),
        ("PlanLimits", &["requests_per_minute"][..]),
    ] {
        let definition = &document["components"]["schemas"][schema];
        let required = names(&definition["required"]);
        for field in fields {
            assert!(
                required.contains(*field),
                "{schema}.{field} must always be present"
            );
            assert!(
                split_null(&definition["properties"][field]).0,
                "{schema}.{field} must accept null"
            );
        }
    }
}
