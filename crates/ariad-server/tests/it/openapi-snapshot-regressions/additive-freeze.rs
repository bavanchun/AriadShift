use super::*;

fn contract() -> Value {
    let mut value = crud(object(&[], json!({})), object(&[], json!({})));
    let item = value["paths"]
        .as_object_mut()
        .unwrap()
        .remove("/things")
        .unwrap();
    value["paths"]["/v1/things"] = item;
    value
}

#[test]
fn identical_contract_passes_with_and_without_refresh() {
    let old = contract();
    for refresh in [false, true] {
        assert!(snapshot_changes(&old, &old, refresh).is_empty());
    }
}

#[test]
fn ordinary_additions_require_explicit_snapshot_refresh() {
    let old = contract();
    let mut new = old.clone();
    new["paths"]["/v1/more"] = json!({"get": {"responses": {"200": {"description": "ok"}}}});
    assert!(breaking_changes(&old, &new).is_empty());
    assert!(!snapshot_changes(&old, &new, false).is_empty());
    assert!(snapshot_changes(&old, &new, true).is_empty());
}

#[test]
fn breaking_changes_are_rejected_even_during_refresh() {
    let old = contract();
    let mut new = old.clone();
    new["paths"].as_object_mut().unwrap().remove("/v1/things");
    for refresh in [false, true] {
        assert!(!snapshot_changes(&old, &new, refresh).is_empty());
    }
}

fn with_blob() -> Value {
    let mut value = contract();
    value["paths"]["/v1/blobs/{token}"] = json!({"put": {
        "requestBody": {"content": {"application/octet-stream": {"schema": {"$ref": "#/components/schemas/BlobOnly"}}}},
        "responses": {"200": {"description": "stored"}},
        "security": [{"blob_token": []}]
    }});
    value["components"]["schemas"]["BlobOnly"] = json!({"type": "string"});
    value["components"]["securitySchemes"] = json!({
        "blob_token": {"type": "apiKey", "in": "query", "name": "token"},
        "bearer": {"type": "http", "scheme": "bearer"}
    });
    value
}

#[test]
fn blob_only_component_dependencies_are_excluded_transitively() {
    let mut old = with_blob();
    old["components"]["schemas"]["BlobOnly"] =
        json!({"type": "array", "items": {"$ref": "#/components/schemas/BlobChild"}});
    old["components"]["schemas"]["BlobChild"] = json!({"type": "integer"});
    let mut new = old.clone();
    new["paths"]
        .as_object_mut()
        .unwrap()
        .remove("/v1/blobs/{token}");
    for name in ["BlobOnly", "BlobChild"] {
        new["components"]["schemas"]
            .as_object_mut()
            .unwrap()
            .remove(name);
    }
    new["components"]["securitySchemes"]
        .as_object_mut()
        .unwrap()
        .remove("blob_token");
    assert!(snapshot_changes(&old, &new, false).is_empty());

    // An ordinary component also uses the child, so its contract must survive.
    old["components"]["schemas"]["Req"]["properties"]["shared"] =
        json!({"$ref": "#/components/schemas/BlobChild"});
    new["components"]["schemas"]["Req"] = old["components"]["schemas"]["Req"].clone();
    assert!(!snapshot_changes(&old, &new, true).is_empty());
}

#[test]
fn blob_only_operations_and_exclusive_components_are_outside_the_freeze() {
    let old = with_blob();
    let mut new = old.clone();
    new["paths"]
        .as_object_mut()
        .unwrap()
        .remove("/v1/blobs/{token}");
    new["components"]["schemas"]
        .as_object_mut()
        .unwrap()
        .remove("BlobOnly");
    new["components"]["securitySchemes"]
        .as_object_mut()
        .unwrap()
        .remove("blob_token");
    assert_compatible(&old, &new);
    assert!(snapshot_changes(&old, &new, false).is_empty());
    let mut added = old.clone();
    added["paths"]["/v1/blobs/{token}"]["put"]["responses"]["429"] = json!({"description": "busy"});
    added["components"]["schemas"]["BlobOnly"]["maxLength"] = json!(12);
    assert!(snapshot_changes(&old, &added, false).is_empty());
}

#[test]
fn schemas_and_security_shared_with_blob_operations_remain_frozen() {
    let mut old = with_blob();
    old["paths"]["/v1/blobs/{token}"]["put"]["requestBody"]["content"]["application/octet-stream"]
        ["schema"] = json!({"$ref": "#/components/schemas/Res"});
    old["components"]["schemas"]["Res"] = object(&["id"], json!({"id": {"type": "string"}}));
    old["paths"]["/v1/blobs/{token}"]["put"]["security"] = json!([{"bearer": []}]);
    let mut new = old.clone();
    new["components"]["schemas"]["Res"]["properties"]
        .as_object_mut()
        .unwrap()
        .remove("id");
    assert!(!snapshot_changes(&old, &new, true).is_empty());
    let mut new = old.clone();
    new["components"]["securitySchemes"]["bearer"]["scheme"] = json!("basic");
    assert!(!snapshot_changes(&old, &new, true).is_empty());
}
