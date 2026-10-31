use super::*;

fn property_doc(schema: Value, dir: Dir) -> Value {
    let value = object(&["value"], json!({"value": schema}));
    let empty = object(&[], json!({}));
    match dir {
        Dir::Request => crud(value, empty),
        Dir::Response => crud(empty, value),
    }
}

fn rejects_at(old: &Value, new: &Value, path: &str, constraint: &str) {
    let problems = breaking_changes(old, new);
    assert!(
        problems
            .iter()
            .any(|p| p.contains(path) && p.contains(constraint)),
        "expected {constraint} at {path}, got {problems:?}"
    );
}

#[test]
fn composite_branches_follow_request_and_response_direction() {
    for composite in ["oneOf", "anyOf", "allOf"] {
        for dir in [Dir::Request, Dir::Response] {
            let wrap = |part: Value| {
                let mut value = json!({});
                value[composite] = json!([part]);
                property_doc(value, dir)
            };
            let narrow = wrap(json!({"type": "string", "maxLength": 8}));
            let wide = wrap(json!({"type": "string", "maxLength": 16}));
            let path = match dir {
                Dir::Request => "Req.value",
                Dir::Response => "Res.value",
            };
            let (compatible_old, compatible_new, broken_old, broken_new) = match dir {
                Dir::Request => (&narrow, &wide, &wide, &narrow),
                Dir::Response => (&wide, &narrow, &narrow, &wide),
            };
            assert_compatible(compatible_old, compatible_new);
            rejects_at(
                broken_old,
                broken_new,
                &format!("{path}.{composite}[0]"),
                "maxLength",
            );
        }
    }
}

#[test]
fn composite_siblings_and_new_composites_are_checked() {
    let old = property_doc(
        json!({"allOf": [{"type": "string"}], "maxLength": 16}),
        Dir::Request,
    );
    let new = property_doc(
        json!({"allOf": [{"type": "string"}], "maxLength": 8}),
        Dir::Request,
    );
    rejects_at(&old, &new, "Req.value", "maxLength");
    let old = property_doc(json!({"type": "string"}), Dir::Request);
    let new = property_doc(
        json!({"allOf": [{"type": "string", "maxLength": 8}]}),
        Dir::Request,
    );
    rejects_at(&old, &new, "Req.value", "composite");
}

#[test]
fn unsupported_validation_keywords_cannot_change_inside_composite_branches() {
    for (key, first, changed, kind) in [
        ("const", json!("fixed"), json!("other"), "string"),
        (
            "not",
            json!({"const": "blocked"}),
            json!({"const": "other"}),
            "string",
        ),
        (
            "propertyNames",
            json!({"pattern": "^[a-z]+$"}),
            json!({"maxLength": 8}),
            "object",
        ),
    ] {
        let plain = json!({"type": kind});
        let mut constrained = plain.clone();
        constrained[key] = first;
        let mut different = plain.clone();
        different[key] = changed;
        let wrap = |schema: Value, dir| property_doc(json!({"anyOf": [schema]}), dir);
        for dir in [Dir::Request, Dir::Response] {
            let path = if dir == Dir::Request {
                "Req.value.anyOf[0]"
            } else {
                "Res.value.anyOf[0]"
            };
            // Unsupported semantics are intentionally conservative in either
            // direction: additions, removals and value changes need review.
            for (old, new) in [
                (&plain, &constrained),
                (&constrained, &plain),
                (&constrained, &different),
            ] {
                rejects_at(&wrap(old.clone(), dir), &wrap(new.clone(), dir), path, key);
            }
            assert_compatible(
                &wrap(constrained.clone(), dir),
                &wrap(constrained.clone(), dir),
            );
        }
    }
}

#[test]
fn unchanged_unsupported_validation_allows_descriptive_metadata_changes() {
    for dir in [Dir::Request, Dir::Response] {
        let schema = json!({"type": "object", "propertyNames": {"pattern": "^[a-z]+$"}});
        let mut described = schema.clone();
        described.as_object_mut().unwrap().extend(obj(&json!({
            "title": "Readable name",
            "description": "Updated explanation",
            "example": {"key": "value"},
            "examples": [{"key": "value"}],
            "$comment": "Maintainer context",
            "deprecated": true,
            "externalDocs": {"url": "https://example.com/docs"}
        })));
        let wrap = |schema: Value| property_doc(json!({"anyOf": [schema]}), dir);
        let old = wrap(schema);
        let new = wrap(described);
        assert_compatible(&old, &new);
        assert_compatible(&new, &old);
        assert!(!snapshot_changes(&old, &new, false).is_empty());
        assert!(snapshot_changes(&old, &new, true).is_empty());
    }
}

#[test]
fn boolean_composite_branches_preserve_directional_constraints() {
    for other in [json!({"type": "string"}), json!({"type": "object"})] {
        let wrap =
            |boolean: bool, dir| property_doc(json!({"anyOf": [boolean, other.clone()]}), dir);
        rejects_at(
            &wrap(false, Dir::Response),
            &wrap(true, Dir::Response),
            "Res.value.anyOf[0]",
            "schema",
        );
        rejects_at(
            &wrap(true, Dir::Request),
            &wrap(false, Dir::Request),
            "Req.value.anyOf[0]",
            "schema",
        );
        assert_compatible(&wrap(true, Dir::Response), &wrap(false, Dir::Response));
        assert_compatible(&wrap(false, Dir::Request), &wrap(true, Dir::Request));
    }
}

#[test]
fn boolean_schemas_compare_with_object_schemas_without_losing_constraints() {
    for shape in [
        json!({"type": "string"}),
        json!({"type": "object", "required": ["id"], "properties": {"id": {"type": "string"}}}),
    ] {
        let wrap = |schema: Value, dir| property_doc(json!({"anyOf": [schema]}), dir);
        rejects_at(
            &wrap(shape.clone(), Dir::Response),
            &wrap(json!(true), Dir::Response),
            "Res.value.anyOf[0]",
            "type",
        );
        rejects_at(
            &wrap(json!(true), Dir::Request),
            &wrap(shape.clone(), Dir::Request),
            "Req.value.anyOf[0]",
            "type",
        );
        rejects_at(
            &wrap(json!(false), Dir::Response),
            &wrap(shape.clone(), Dir::Response),
            "Res.value.anyOf[0]",
            "schema",
        );
        rejects_at(
            &wrap(shape.clone(), Dir::Request),
            &wrap(json!(false), Dir::Request),
            "Req.value.anyOf[0]",
            "schema",
        );
        assert_compatible(
            &wrap(json!(true), Dir::Response),
            &wrap(shape.clone(), Dir::Response),
        );
        if shape.get("properties").is_some() {
            // Request widening must still preserve published property names.
            rejects_at(
                &wrap(shape.clone(), Dir::Request),
                &wrap(json!(true), Dir::Request),
                "Req.value.anyOf[0].id",
                "removed property",
            );
        } else {
            assert_compatible(
                &wrap(shape.clone(), Dir::Request),
                &wrap(json!(true), Dir::Request),
            );
        }
        assert_compatible(
            &wrap(shape.clone(), Dir::Response),
            &wrap(json!(false), Dir::Response),
        );
        assert_compatible(
            &wrap(json!(false), Dir::Request),
            &wrap(shape, Dir::Request),
        );
    }
    for dir in [Dir::Request, Dir::Response] {
        assert_compatible(
            &property_doc(json!(true), dir),
            &property_doc(json!({}), dir),
        );
        assert_compatible(
            &property_doc(json!({}), dir),
            &property_doc(json!(true), dir),
        );
    }
}

#[test]
fn nested_items_and_additional_properties_preserve_directional_constraints() {
    for (key, kind, nested_path) in [
        ("items", "array", "[]"),
        ("additionalProperties", "object", ".*"),
    ] {
        let mut constrained = json!({"type": kind});
        constrained[key] = json!({"type": "string"});
        for unconstrained in [None, Some(json!(true))] {
            let mut plain = json!({"type": kind});
            if let Some(value) = unconstrained {
                plain[key] = value;
            }
            // Exercise nested comparison through composite branch coverage.
            let wrap = |schema: Value, dir| property_doc(json!({"anyOf": [schema]}), dir);
            rejects_at(
                &wrap(constrained.clone(), Dir::Response),
                &wrap(plain.clone(), Dir::Response),
                &format!("Res.value.anyOf[0]{nested_path}"),
                "schema",
            );
            rejects_at(
                &wrap(plain.clone(), Dir::Request),
                &wrap(constrained.clone(), Dir::Request),
                &format!("Req.value.anyOf[0]{nested_path}"),
                "schema",
            );
            assert_compatible(
                &wrap(plain.clone(), Dir::Response),
                &wrap(constrained.clone(), Dir::Response),
            );
            assert_compatible(
                &wrap(constrained.clone(), Dir::Request),
                &wrap(plain.clone(), Dir::Request),
            );
        }
    }
}

fn referenced_text(siblings: Value, dir: Dir) -> Value {
    let mut branch = json!({"$ref": "#/components/schemas/Text"});
    branch.as_object_mut().unwrap().extend(obj(&siblings));
    let mut value = property_doc(json!({"anyOf": [branch]}), dir);
    value["components"]["schemas"]["Text"] = json!({"type": "string"});
    value
}

#[test]
fn constraints_beside_branch_references_are_checked_in_both_directions() {
    for (key, value) in [("maxLength", json!(8)), ("pattern", json!("^[a-z]+$"))] {
        let mut constrained = json!({});
        constrained[key] = value;
        rejects_at(
            &referenced_text(json!({}), Dir::Request),
            &referenced_text(constrained.clone(), Dir::Request),
            "Req.value.anyOf[0]",
            key,
        );
        rejects_at(
            &referenced_text(constrained.clone(), Dir::Response),
            &referenced_text(json!({}), Dir::Response),
            "Res.value.anyOf[0]",
            key,
        );
        assert_compatible(
            &referenced_text(constrained.clone(), Dir::Request),
            &referenced_text(json!({}), Dir::Request),
        );
        assert_compatible(
            &referenced_text(json!({}), Dir::Response),
            &referenced_text(constrained, Dir::Response),
        );
    }
}

#[test]
fn repeated_references_do_not_skip_occurrence_specific_constraints() {
    for dir in [Dir::Request, Dir::Response] {
        let mut wide = property_doc(
            object(
                &["first", "second"],
                json!({
                    "first": {"$ref": "#/components/schemas/Text"},
                    "second": {"$ref": "#/components/schemas/Text"}
                }),
            ),
            dir,
        );
        wide["components"]["schemas"]["Text"] = json!({"type": "string"});
        let mut narrow = wide.clone();
        let component = if dir == Dir::Request { "Req" } else { "Res" };
        narrow["components"]["schemas"][component]["properties"]["value"]["properties"]["second"]
            ["maxLength"] = json!(8);
        let (old, new) = match dir {
            Dir::Request => (&wide, &narrow),
            Dir::Response => (&narrow, &wide),
        };
        rejects_at(old, new, &format!("{component}.value.second"), "maxLength");
        assert_compatible(new, old);
    }
}

#[test]
fn union_branch_insertion_and_removal_follow_direction() {
    for composite in ["anyOf", "oneOf"] {
        let wrap = |branches: Value, dir| {
            let mut schema = json!({});
            schema[composite] = branches;
            property_doc(schema, dir)
        };
        let narrow = json!([{"type": "string"}]);
        let wide = json!([{"type": "string"}, {"type": "integer"}]);
        for dir in [Dir::Request, Dir::Response] {
            let narrow = wrap(narrow.clone(), dir);
            let wide = wrap(wide.clone(), dir);
            match dir {
                Dir::Request => {
                    assert_compatible(&narrow, &wide);
                    rejects_at(&wide, &narrow, "Req.value", composite);
                }
                Dir::Response => {
                    assert_compatible(&wide, &narrow);
                    rejects_at(&narrow, &wide, "Res.value", composite);
                }
            }
        }
    }
}

#[test]
fn conjunction_branch_insertion_and_removal_follow_direction() {
    let wrap = |branches: Value, dir| property_doc(json!({"allOf": branches}), dir);
    let wide = json!([{"type": "string"}]);
    let narrow = json!([{"type": "string"}, {"maxLength": 8}]);
    assert_compatible(
        &wrap(narrow.clone(), Dir::Request),
        &wrap(wide.clone(), Dir::Request),
    );
    assert_compatible(
        &wrap(wide.clone(), Dir::Response),
        &wrap(narrow.clone(), Dir::Response),
    );
    rejects_at(
        &wrap(wide.clone(), Dir::Request),
        &wrap(narrow.clone(), Dir::Request),
        "Req.value",
        "allOf",
    );
    rejects_at(
        &wrap(narrow, Dir::Response),
        &wrap(wide, Dir::Response),
        "Res.value",
        "allOf",
    );
}

#[test]
fn overlapping_one_of_changes_cannot_masquerade_as_safe_union_changes() {
    let string = json!({"type": "string"});
    let short_string = json!({"type": "string", "maxLength": 8});
    let one = |branches: Value, dir| property_doc(json!({"oneOf": branches}), dir);
    // Adding the overlapping alternative makes short strings invalid requests.
    rejects_at(
        &one(json!([string.clone()]), Dir::Request),
        &one(json!([string.clone(), short_string.clone()]), Dir::Request),
        "Req.value",
        "oneOf",
    );
    // Removing it makes previously excluded short strings possible responses.
    rejects_at(
        &one(json!([string.clone(), short_string]), Dir::Response),
        &one(json!([string]), Dir::Response),
        "Res.value",
        "oneOf",
    );
}

#[test]
fn one_of_overlap_checks_follow_unchanged_component_references() {
    let mut old = property_doc(
        json!({"oneOf": [
            {"$ref": "#/components/schemas/Text"},
            {"$ref": "#/components/schemas/Other"}
        ]}),
        Dir::Request,
    );
    old["components"]["schemas"]["Text"] = json!({"type": "string"});
    old["components"]["schemas"]["Other"] = json!({"type": "integer"});
    let mut new = old.clone();
    new["components"]["schemas"]["Other"] = json!({"type": ["integer", "string"]});
    rejects_at(&old, &new, "Req.value", "oneOf");
}

#[test]
fn reordered_composite_branches_preserve_compatibility() {
    for composite in ["oneOf", "anyOf", "allOf"] {
        for dir in [Dir::Request, Dir::Response] {
            let wrap = |branches: Value| {
                let mut schema = json!({});
                schema[composite] = branches;
                property_doc(schema, dir)
            };
            assert_compatible(
                &wrap(json!([{"type": "string"}, {"type": "integer"}])),
                &wrap(json!([{"type": "integer"}, {"type": "string"}])),
            );
        }
    }
}

#[test]
fn response_enum_removal_is_breaking_but_request_removal_is_compatible() {
    let constrained = json!({"type": "string", "enum": ["a", "b"]});
    let plain = json!({"type": "string"});
    rejects_at(
        &property_doc(constrained.clone(), Dir::Response),
        &property_doc(plain.clone(), Dir::Response),
        "Res.value",
        "enum",
    );
    assert_compatible(
        &property_doc(constrained, Dir::Request),
        &property_doc(plain, Dir::Request),
    );
}

#[test]
fn request_defaults_cannot_change_or_disappear() {
    let old = property_doc(json!({"type": "integer", "default": 50}), Dir::Request);
    for next in [
        json!({"type": "integer", "default": 10}),
        json!({"type": "integer"}),
    ] {
        rejects_at(
            &old,
            &property_doc(next, Dir::Request),
            "Req.value",
            "default",
        );
    }
    assert_compatible(
        &property_doc(json!({"type": "integer"}), Dir::Request),
        &old,
    );
    assert_compatible(
        &property_doc(json!({"type": "integer", "default": 50}), Dir::Response),
        &property_doc(json!({"type": "integer", "default": 10}), Dir::Response),
    );
}

#[test]
fn exclusive_bounds_follow_direction_and_allow_compatible_changes() {
    for (key, narrow, wide) in [("exclusiveMaximum", 8, 16), ("exclusiveMinimum", 16, 8)] {
        let schema = |bound| {
            let mut s = json!({"type": "number"});
            s[key] = json!(bound);
            s
        };
        let plain = json!({"type": "number"});
        rejects_at(
            &property_doc(plain.clone(), Dir::Request),
            &property_doc(schema(narrow), Dir::Request),
            "Req.value",
            key,
        );
        rejects_at(
            &property_doc(schema(narrow), Dir::Response),
            &property_doc(plain.clone(), Dir::Response),
            "Res.value",
            key,
        );
        assert_compatible(
            &property_doc(schema(narrow), Dir::Request),
            &property_doc(plain.clone(), Dir::Request),
        );
        assert_compatible(
            &property_doc(plain, Dir::Response),
            &property_doc(schema(narrow), Dir::Response),
        );
        for dir in [Dir::Request, Dir::Response] {
            let narrow = property_doc(schema(narrow), dir);
            let wide = property_doc(schema(wide), dir);
            match dir {
                Dir::Request => {
                    assert_compatible(&narrow, &wide);
                    rejects_at(&wide, &narrow, "Req.value", key);
                }
                Dir::Response => {
                    assert_compatible(&wide, &narrow);
                    rejects_at(&narrow, &wide, "Res.value", key);
                }
            }
        }
    }
}

#[test]
fn patterns_multiples_and_uniqueness_follow_direction() {
    for (key, first, changed, kind) in [
        ("pattern", json!("^[a-z]+$"), json!("^[A-Z]+$"), "string"),
        ("multipleOf", json!(2), json!(3), "number"),
        ("uniqueItems", json!(true), json!(false), "array"),
    ] {
        let mut plain = json!({"type": kind});
        if kind == "array" {
            plain["items"] = json!({"type": "string"});
        }
        let mut constrained = plain.clone();
        constrained[key] = first;
        let mut different = plain.clone();
        different[key] = changed;
        rejects_at(
            &property_doc(plain.clone(), Dir::Request),
            &property_doc(constrained.clone(), Dir::Request),
            "Req.value",
            key,
        );
        rejects_at(
            &property_doc(constrained.clone(), Dir::Response),
            &property_doc(plain.clone(), Dir::Response),
            "Res.value",
            key,
        );
        if key != "uniqueItems" {
            for dir in [Dir::Request, Dir::Response] {
                let path = if dir == Dir::Request {
                    "Req.value"
                } else {
                    "Res.value"
                };
                rejects_at(
                    &property_doc(constrained.clone(), dir),
                    &property_doc(different.clone(), dir),
                    path,
                    key,
                );
            }
        }
        assert_compatible(
            &property_doc(constrained.clone(), Dir::Request),
            &property_doc(plain.clone(), Dir::Request),
        );
        assert_compatible(
            &property_doc(plain, Dir::Response),
            &property_doc(constrained, Dir::Response),
        );
    }
}

#[test]
fn security_scheme_removal_and_wire_changes_are_breaking() {
    let mut old = crud(object(&[], json!({})), object(&[], json!({})));
    old["components"]["securitySchemes"] =
        json!({"bearer": {"type": "http", "scheme": "bearer", "description": "old"}});
    let mut removed = old.clone();
    removed["components"]["securitySchemes"]
        .as_object_mut()
        .unwrap()
        .remove("bearer");
    rejects_at(&old, &removed, "bearer", "security scheme");
    let mut changed = old.clone();
    changed["components"]["securitySchemes"]["bearer"]["scheme"] = json!("basic");
    rejects_at(&old, &changed, "bearer", "security scheme");
    let mut description = old.clone();
    description["components"]["securitySchemes"]["bearer"]["description"] = json!("new");
    assert_compatible(&old, &description);
    let mut added = old.clone();
    added["components"]["securitySchemes"]["another"] =
        json!({"type": "apiKey", "in": "header", "name": "x-key"});
    assert_compatible(&old, &added);
}

#[test]
fn adding_required_security_scopes_is_breaking() {
    let mut old = crud(object(&[], json!({})), object(&[], json!({})));
    old["paths"]["/things"]["post"]["security"] = json!([{"oauth": ["read"]}]);
    old["components"]["securitySchemes"] = json!({"oauth": {"type": "oauth2", "flows": {"clientCredentials": {"tokenUrl": "https://example.com/token", "scopes": {"read": "read", "write": "write"}}}}});
    let mut new = old.clone();
    new["paths"]["/things"]["post"]["security"] = json!([{"oauth": ["read", "write"]}]);
    rejects_at(&old, &new, "POST /things", "security");
    assert_compatible(&old, &old);
    // Requiring fewer scopes accepts every previously valid credential.
    assert_compatible(&new, &old);
}
