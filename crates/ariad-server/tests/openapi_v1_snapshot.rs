//! `/v1` is frozen: the generated OpenAPI document may only grow.
//!
//! `tests/snapshots/openapi_v1.json` is the frozen contract. The current
//! document must stay backward compatible with it: no removed paths,
//! operations, response codes, media types, headers, properties, parameters
//! or schemas; no narrowed request types or newly required request fields;
//! no widened response types or response properties that stop being
//! required; no tightened security. New paths, operations, response codes,
//! optional request fields, response properties and enum values are
//! additive and pass compatibility checks, but must be frozen before shipping.
//! Blob transfer operations and components used exclusively by them are excluded.
//!
//! After an additive change, refresh the snapshot with
//! `ARIAD_UPDATE_OPENAPI_BASELINE=1 cargo nextest run -p ariad-server --test openapi_v1_snapshot`.
//! The update refuses breaking changes; those need a new major version path.

use std::{
    collections::{BTreeSet, HashSet},
    fs,
    path::PathBuf,
};

use serde_json::{Map, Value, json};

#[path = "openapi-snapshot-regressions/additive-freeze.rs"]
mod additive_freeze;
#[path = "openapi-snapshot-regressions/compatibility-constraints.rs"]
mod compatibility_constraints;
#[path = "openapi-snapshot-regressions/nullable-fields.rs"]
mod nullable_fields;

const METHODS: [&str; 8] = [
    "get", "put", "post", "delete", "patch", "head", "options", "trace",
];

fn baseline_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots/openapi_v1.json")
}

fn current() -> Value {
    let json = ariad_server::api::openapi_json().expect("serialize the OpenAPI document");
    serde_json::from_str(&json).expect("parse the OpenAPI document")
}

#[test]
fn v1_stays_backward_compatible_with_the_frozen_snapshot() {
    let path = baseline_path();
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("could not read {}: {error}", path.display()));
    let baseline: Value = serde_json::from_str(&text).expect("parse the snapshot");
    let problems = snapshot_changes(
        &baseline,
        &current(),
        std::env::var_os("ARIAD_UPDATE_OPENAPI_BASELINE").is_some(),
    );
    assert!(
        problems.is_empty(),
        "/v1 snapshot check failed (keep the old shape, add a new major version path, or refresh a compatible change):\n  {}",
        problems.join("\n  ")
    );
    if std::env::var_os("ARIAD_UPDATE_OPENAPI_BASELINE").is_some() {
        let json = ariad_server::api::openapi_json().expect("serialize");
        fs::write(&path, json).expect("write the snapshot");
    }
}

fn snapshot_changes(old: &Value, new: &Value, refresh: bool) -> Vec<String> {
    let mut problems = breaking_changes(old, new);
    if problems.is_empty() && !refresh && frozen_projection(old) != frozen_projection(new) {
        problems.push(
            "unfrozen change: refresh it with ARIAD_UPDATE_OPENAPI_BASELINE=1 cargo nextest run -p ariad-server --test openapi_v1_snapshot"
                .to_owned(),
        );
    }
    problems
}

fn component_pointer(kind: &str, name: &str) -> String {
    format!(
        "/components/{}/{}",
        kind.replace('~', "~0").replace('/', "~1"),
        name.replace('~', "~0").replace('/', "~1")
    )
}

/// Local component references, including security requirements whose names are
/// implicit references to `securitySchemes` rather than `$ref` values.
fn component_references(value: &Value, references: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            if let Some(reference) = map.get("$ref").and_then(Value::as_str)
                && let Some(pointer) = reference.strip_prefix('#')
                && pointer.starts_with("/components/")
            {
                // A reference into a component's child still depends on the
                // containing component and all of its own dependencies.
                references.insert(pointer.split('/').take(4).collect::<Vec<_>>().join("/"));
            }
            if let Some(requirements) = map.get("security").and_then(Value::as_array) {
                for requirement in requirements {
                    for name in obj(requirement).keys() {
                        references.insert(component_pointer("securitySchemes", name));
                    }
                }
            }
            for child in map.values() {
                component_references(child, references);
            }
        }
        Value::Array(items) => {
            for child in items {
                component_references(child, references);
            }
        }
        _ => {}
    }
}

fn reachable_components(document: &Value, roots: &Value) -> BTreeSet<String> {
    let mut references = BTreeSet::new();
    component_references(roots, &mut references);
    let mut visited = BTreeSet::new();
    while let Some(reference) = references.iter().find(|r| !visited.contains(*r)).cloned() {
        visited.insert(reference.clone());
        if let Some(component) = document.pointer(&reference) {
            component_references(component, &mut references);
        }
    }
    references
}

/// Preserve the generated document for drift checks, but freeze only the API
/// contract. Components shared with a normal operation or another retained
/// component remain protected, including transitive references and security.
fn frozen_projection(document: &Value) -> Value {
    let mut projected = document.clone();
    let blob = projected["paths"]
        .as_object_mut()
        .and_then(|paths| paths.remove("/v1/blobs/{token}"));
    let blob = blob.unwrap_or(Value::Null);
    let blob_components = reachable_components(document, &blob);
    let mut retained = projected.clone();
    retained.as_object_mut().unwrap().remove("components");
    // Unused ordinary components also belong to the public contract. Treat them
    // as roots so their references into a blob dependency cannot be removed.
    for (kind, entries) in obj(&document["components"]) {
        for (name, component) in obj(&entries) {
            let pointer = component_pointer(&kind, &name);
            if !blob_components.contains(&pointer) {
                retained["components"][&kind][&name] = component;
            }
        }
    }
    let shared = reachable_components(document, &retained);
    for (kind, entries) in obj(&document["components"]) {
        for name in obj(&entries).keys() {
            let pointer = component_pointer(&kind, name);
            if blob_components.contains(&pointer) && !shared.contains(&pointer) {
                projected["components"][&kind]
                    .as_object_mut()
                    .unwrap()
                    .remove(name);
            }
        }
    }
    if let Some(components) = projected["components"].as_object_mut() {
        components.retain(|_, entries| !entries.as_object().is_some_and(Map::is_empty));
    }
    projected
}

#[test]
fn v1_is_marked_stable() {
    assert_eq!(current()["info"]["x-stability"], "stable");
    let text = fs::read_to_string(baseline_path()).expect("read the snapshot");
    let baseline: Value = serde_json::from_str(&text).expect("parse the snapshot");
    assert_eq!(baseline["info"]["x-stability"], "stable");
}

#[test]
fn the_current_document_is_compatible_with_itself() {
    let doc = current();
    assert_eq!(breaking_changes(&doc, &doc), Vec::<String>::new());
}

#[test]
fn removing_a_response_field_is_breaking() {
    let old = current();
    let mut new = old.clone();
    let job = &mut new["components"]["schemas"]["Job"];
    job["properties"]
        .as_object_mut()
        .expect("Job properties")
        .remove("status")
        .expect("Job.status exists");
    job["required"]
        .as_array_mut()
        .expect("Job required")
        .retain(|name| name != "status");
    let problems = breaking_changes(&old, &new);
    assert!(
        problems.iter().any(|p| p.contains("Job.status")),
        "{problems:?}"
    );
}

#[test]
fn removing_paths_operations_and_response_codes_is_breaking() {
    let old = current();

    let mut new = old.clone();
    new["paths"]
        .as_object_mut()
        .unwrap()
        .remove("/v1/limits")
        .expect("limits path");
    assert_mentions(&old, &new, "removed path /v1/limits");

    let mut new = old.clone();
    new["paths"]["/v1/uploads/{id}"]
        .as_object_mut()
        .unwrap()
        .remove("delete")
        .expect("upload delete");
    assert_mentions(&old, &new, "removed operation DELETE /v1/uploads/{id}");

    let mut new = old.clone();
    new["paths"]["/v1/jobs/{id}/outputs"]["get"]["responses"]
        .as_object_mut()
        .unwrap()
        .remove("410")
        .expect("410 response");
    assert_mentions(
        &old,
        &new,
        "GET /v1/jobs/{id}/outputs: removed response 410",
    );
}

fn doc(paths: Value, schemas: Value) -> Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "t", "version": "1" },
        "paths": paths,
        "components": { "schemas": schemas },
    })
}

/// One `POST /things` taking `Req` and returning `Res`.
fn crud(request: Value, response: Value) -> Value {
    doc(
        json!({
            "/things": {
                "post": {
                    "operationId": "create_thing",
                    "requestBody": {
                        "required": true,
                        "content": { "application/json": { "schema": { "$ref": "#/components/schemas/Req" } } }
                    },
                    "responses": {
                        "201": {
                            "description": "created",
                            "content": { "application/json": { "schema": { "$ref": "#/components/schemas/Res" } } }
                        }
                    },
                    "security": [{}, { "bearer": [] }]
                }
            }
        }),
        json!({ "Req": request, "Res": response }),
    )
}

fn object(required: &[&str], properties: Value) -> Value {
    json!({ "type": "object", "required": required, "properties": properties })
}

fn assert_mentions(old: &Value, new: &Value, needle: &str) {
    let problems = breaking_changes(old, new);
    assert!(
        problems.iter().any(|p| p.contains(needle)),
        "expected a problem containing {needle:?}, got {problems:?}"
    );
}

fn assert_compatible(old: &Value, new: &Value) {
    assert_eq!(breaking_changes(old, new), Vec::<String>::new());
}

#[test]
fn request_changes_are_checked() {
    let base = object(
        &["name"],
        json!({
            "name": { "type": "string", "maxLength": 255 },
            "size": { "type": ["integer", "null"], "format": "int64" },
            "mode": { "type": "string", "enum": ["a", "b"] }
        }),
    );
    let res = object(&["id"], json!({ "id": { "type": "string" } }));
    let old = crud(base.clone(), res.clone());

    // A new optional field and a new enum value are additive.
    let mut new = base.clone();
    new["properties"]["note"] = json!({ "type": "string" });
    new["properties"]["mode"]["enum"] = json!(["a", "b", "c"]);
    assert_compatible(&old, &crud(new, res.clone()));

    // A new required field.
    let mut new = base.clone();
    new["properties"]["note"] = json!({ "type": "string" });
    new["required"] = json!(["name", "note"]);
    assert_mentions(&old, &crud(new, res.clone()), "Req.note: became required");

    // Narrowed types: nullability, enum values, length, type.
    let mut new = base.clone();
    new["properties"]["size"]["type"] = json!("integer");
    assert_mentions(
        &old,
        &crud(new, res.clone()),
        "Req.size: no longer accepts null",
    );
    let mut new = base.clone();
    new["properties"]["mode"]["enum"] = json!(["a"]);
    assert_mentions(
        &old,
        &crud(new, res.clone()),
        "Req.mode: removed enum value \"b\"",
    );
    let mut new = base.clone();
    new["properties"]["name"]["maxLength"] = json!(64);
    assert_mentions(
        &old,
        &crud(new, res.clone()),
        "Req.name: maxLength tightened",
    );
    let mut new = base.clone();
    new["properties"]["name"]["type"] = json!("integer");
    assert_mentions(&old, &crud(new, res.clone()), "Req.name: type narrowed");

    // Removed fields and a body that stops accepting unknown fields.
    let mut new = base.clone();
    new["properties"].as_object_mut().unwrap().remove("mode");
    assert_mentions(&old, &crud(new, res.clone()), "Req.mode: removed property");
    let mut new = base;
    new["additionalProperties"] = json!(false);
    assert_mentions(
        &old,
        &crud(new, res),
        "Req: no longer accepts unknown properties",
    );
}

#[test]
fn response_changes_are_checked() {
    let req = object(&[], json!({ "name": { "type": "string" } }));
    let base = object(
        &["id", "status"],
        json!({
            "id": { "type": "string", "format": "uuid" },
            "status": { "$ref": "#/components/schemas/Status" },
            "error": { "oneOf": [{ "$ref": "#/components/schemas/Status" }, { "type": "null" }] }
        }),
    );
    let status = json!({ "type": "string", "enum": ["queued", "done"] });
    let with_status = |res: Value, status: Value| {
        let mut d = crud(req.clone(), res);
        d["components"]["schemas"]["Status"] = status;
        d
    };
    let old = with_status(base.clone(), status.clone());

    // New properties and enum values are additive.
    let mut new = base.clone();
    new["properties"]["extra"] = json!({ "type": "integer" });
    assert_compatible(
        &old,
        &with_status(
            new,
            json!({ "type": "string", "enum": ["queued", "done", "failed"] }),
        ),
    );

    // A required property becomes optional or nullable.
    let mut new = base.clone();
    new["required"] = json!(["id"]);
    assert_mentions(
        &old,
        &with_status(new, status.clone()),
        "Res.status: no longer always present",
    );
    let mut new = base.clone();
    new["properties"]["id"]["type"] = json!(["string", "null"]);
    assert_mentions(
        &old,
        &with_status(new, status.clone()),
        "Res.id: may now be null",
    );

    // Widened or changed types.
    let mut new = base.clone();
    new["properties"]["id"]["format"] = json!("uri");
    assert_mentions(
        &old,
        &with_status(new, status.clone()),
        "Res.id: format changed",
    );
    let mut new = base.clone();
    new["properties"]["id"]["type"] = json!(["string", "integer"]);
    assert_mentions(
        &old,
        &with_status(new, status.clone()),
        "Res.id: type widened",
    );

    // A nested schema through a nullable reference.
    assert_mentions(
        &old,
        &with_status(base, json!({ "type": "string", "enum": ["queued"] })),
        "Status: removed enum value \"done\"",
    );
}

#[test]
fn parameters_security_and_identifiers_are_checked() {
    let res = object(&[], json!({}));
    let old = crud(object(&[], json!({})), res.clone());

    let mut new = old.clone();
    new["paths"]["/things"]["post"]["parameters"] = json!([
        { "name": "x-thing", "in": "header", "required": true, "schema": { "type": "string" } }
    ]);
    assert_mentions(&old, &new, "new required parameter header x-thing");

    let mut optional = old.clone();
    optional["paths"]["/things"]["post"]["parameters"] = json!([
        { "name": "x-thing", "in": "header", "required": false, "schema": { "type": "string" } }
    ]);
    assert_compatible(&old, &optional);
    assert_mentions(&optional, &old, "removed parameter header x-thing");

    let mut new = old.clone();
    new["paths"]["/things"]["post"]["security"] = json!([{ "bearer": [] }]);
    assert_mentions(&old, &new, "no longer accepts anonymous callers");

    let mut new = old.clone();
    new["paths"]["/things"]["post"]["operationId"] = json!("make_thing");
    assert_mentions(&old, &new, "operationId changed");

    let mut new = old.clone();
    new["components"]["schemas"]
        .as_object_mut()
        .unwrap()
        .remove("Res");
    new["components"]["schemas"]["Renamed"] = res;
    new["paths"]["/things"]["post"]["responses"]["201"]["content"]["application/json"]["schema"] =
        json!({ "$ref": "#/components/schemas/Renamed" });
    assert_mentions(&old, &new, "removed schema Res");
}

/// Which side of the wire a schema describes.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum Dir {
    /// Sent by clients: the server must keep accepting everything it accepted.
    Request,
    /// Sent by the server: clients must keep understanding everything it sends.
    Response,
}

impl Dir {
    const fn label(self) -> &'static str {
        match self {
            Self::Request => "request",
            Self::Response => "response",
        }
    }
}

/// Every backward-incompatible difference from `old` to `new`.
fn breaking_changes(old: &Value, new: &Value) -> Vec<String> {
    let old = frozen_projection(old);
    let new = frozen_projection(new);
    let mut checker = Checker {
        old: &old,
        new: &new,
        problems: Vec::new(),
        seen: HashSet::new(),
    };
    checker.document();
    checker.problems.sort();
    checker.problems.dedup();
    checker.problems
}

struct Checker<'a> {
    old: &'a Value,
    new: &'a Value,
    problems: Vec<String>,
    /// Reference pairs already compared, which also stops recursion.
    seen: HashSet<(String, String, Dir)>,
}

fn obj(value: &Value) -> Map<String, Value> {
    value.as_object().cloned().unwrap_or_default()
}

fn names(value: &Value) -> BTreeSet<String> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// The schema type set; `None` when the schema accepts any type.
fn types(schema: &Value) -> Option<BTreeSet<String>> {
    match schema.get("type")? {
        Value::String(name) => Some(BTreeSet::from([name.clone()])),
        Value::Array(_) => Some(names(&schema["type"])),
        _ => None,
    }
}

/// Splits `null` out of a schema: `(nullable, schema without null)`.
fn split_null(schema: &Value) -> (bool, Value) {
    let is_null = |variant: &Value| variant.get("type") == Some(&json!("null"));
    for key in ["oneOf", "anyOf"] {
        if let Some(variants) = schema.get(key).and_then(Value::as_array)
            && variants.iter().any(is_null)
        {
            let rest: Vec<Value> = variants.iter().filter(|&v| !is_null(v)).cloned().collect();
            if let [only] = rest.as_slice()
                && schema.as_object().is_some_and(|map| map.len() == 1)
            {
                return (true, only.clone());
            }
            let mut core = schema.clone();
            core[key] = Value::Array(rest);
            return (true, core);
        }
    }
    if let Some(mut set) = types(schema)
        && set.remove("null")
    {
        let mut core = schema.clone();
        core["type"] = match set.len() {
            1 => json!(set.into_iter().next()),
            _ => json!(set),
        };
        return (true, core);
    }
    (false, schema.clone())
}

/// Whether type set `wide` accepts every value of `narrow`.
fn covers(wide: &BTreeSet<String>, narrow: &BTreeSet<String>) -> bool {
    narrow
        .iter()
        .all(|t| wide.contains(t) || (t == "integer" && wide.contains("number")))
}

/// Alternative security requirements, retaining scopes; an empty object permits
/// anonymous access and an empty requirement list disables inherited security.
fn security(op: &Value, doc: &Value) -> Vec<Value> {
    let requirements = op.get("security").or_else(|| doc.get("security"));
    let Some(requirements) = requirements.and_then(Value::as_array) else {
        return vec![json!({})];
    };
    if requirements.is_empty() {
        vec![json!({})]
    } else {
        requirements.clone()
    }
}

fn accepts_security(new: &Value, old: &Value) -> bool {
    obj(new).iter().all(|(scheme, scopes)| {
        old.get(scheme)
            .is_some_and(|old_scopes| names(scopes).is_subset(&names(old_scopes)))
    })
}

fn without_description(value: &Value) -> Value {
    let mut value = value.clone();
    if let Some(map) = value.as_object_mut() {
        map.remove("description");
    }
    value
}

fn same_branches(old: &[Value], new: &[Value]) -> bool {
    let mut unmatched: Vec<&Value> = new.iter().collect();
    for branch in old {
        let Some(index) = unmatched.iter().position(|candidate| *candidate == branch) else {
            return false;
        };
        unmatched.remove(index);
    }
    unmatched.is_empty()
}

/// A union is safe to treat as `oneOf` only when alternatives cannot overlap.
/// Unknown or object-shaped overlap is deliberately not inferred from position.
fn disjoint_branches(document: &Value, branches: &[Value]) -> bool {
    for (index, left) in branches.iter().enumerate() {
        let (left, _) = Checker::resolve(document, left);
        for right in &branches[index + 1..] {
            let (right, _) = Checker::resolve(document, right);
            if let (Some(a), Some(b)) = (
                left.get("enum").and_then(Value::as_array),
                right.get("enum").and_then(Value::as_array),
            ) && !a.iter().any(|value| b.contains(value))
            {
                continue;
            }
            let (Some(mut a), Some(mut b)) = (types(&left), types(&right)) else {
                return false;
            };
            for set in [&mut a, &mut b] {
                if set.contains("number") {
                    set.insert("integer".to_owned());
                }
            }
            if !a.is_disjoint(&b) {
                return false;
            }
        }
    }
    true
}

/// Path-level and operation-level parameters keyed by location and name.
fn parameters(item: &Value, op: &Value) -> Vec<(String, Value)> {
    let mut out: Vec<(String, Value)> = Vec::new();
    for list in [item.get("parameters"), op.get("parameters")]
        .into_iter()
        .flatten()
        .filter_map(Value::as_array)
    {
        for param in list {
            let location = param["in"].as_str().unwrap_or_default();
            let mut name = param["name"].as_str().unwrap_or_default().to_owned();
            if location == "header" {
                name = name.to_ascii_lowercase();
            }
            let key = format!("{location} {name}");
            out.retain(|(k, _)| *k != key);
            out.push((key, param.clone()));
        }
    }
    out
}

impl Checker<'_> {
    fn problem(&mut self, message: String) {
        self.problems.push(message);
    }

    fn document(&mut self) {
        let old_paths = obj(&self.old["paths"]);
        let new_paths = obj(&self.new["paths"]);
        for (path, old_item) in &old_paths {
            let Some(new_item) = new_paths.get(path) else {
                self.problem(format!("removed path {path}"));
                continue;
            };
            for method in METHODS {
                let Some(old_op) = old_item.get(method) else {
                    continue;
                };
                let at = format!("{} {path}", method.to_ascii_uppercase());
                let Some(new_op) = new_item.get(method) else {
                    self.problem(format!("removed operation {at}"));
                    continue;
                };
                self.operation(&at, (old_item, old_op), (new_item, new_op));
            }
        }
        let new_schemas = obj(&self.new["components"]["schemas"]);
        for name in obj(&self.old["components"]["schemas"]).keys() {
            if !new_schemas.contains_key(name) {
                self.problem(format!("removed schema {name}"));
            }
        }
        let new_schemes = obj(&self.new["components"]["securitySchemes"]);
        for (name, old_scheme) in obj(&self.old["components"]["securitySchemes"]) {
            match new_schemes.get(&name) {
                None => self.problem(format!("removed security scheme {name}")),
                Some(new_scheme)
                    if without_description(&old_scheme) != without_description(new_scheme) =>
                {
                    self.problem(format!("security scheme {name} changed"));
                }
                Some(_) => {}
            }
        }
    }

    fn operation(&mut self, at: &str, old: (&Value, &Value), new: (&Value, &Value)) {
        let (old_item, old_op) = old;
        let (new_item, new_op) = new;
        if old_op.get("operationId") != new_op.get("operationId") {
            self.problem(format!("{at}: operationId changed"));
        }

        let now = security(new_op, self.new);
        for requirement in security(old_op, self.old) {
            if !now
                .iter()
                .any(|candidate| accepts_security(candidate, &requirement))
            {
                let schemes = obj(&requirement);
                let who = if schemes.is_empty() {
                    "anonymous callers".to_owned()
                } else {
                    format!(
                        "{} credentials (security scopes)",
                        schemes.keys().cloned().collect::<Vec<_>>().join("+")
                    )
                };
                self.problem(format!("{at}: no longer accepts {who}"));
            }
        }

        let old_params = parameters(old_item, old_op);
        let new_params = parameters(new_item, new_op);
        for (key, old_param) in &old_params {
            let Some((_, new_param)) = new_params.iter().find(|(k, _)| k == key) else {
                self.problem(format!("{at}: removed parameter {key}"));
                continue;
            };
            if new_param["required"] == json!(true) && old_param["required"] != json!(true) {
                self.problem(format!("{at}: parameter {key} became required"));
            }
            self.schema(
                &format!("{at} parameter {key}"),
                &old_param["schema"],
                &new_param["schema"],
                Dir::Request,
            );
        }
        for (key, new_param) in &new_params {
            if new_param["required"] == json!(true) && !old_params.iter().any(|(k, _)| k == key) {
                self.problem(format!("{at}: new required parameter {key}"));
            }
        }

        match (old_op.get("requestBody"), new_op.get("requestBody")) {
            (Some(_), None) => self.problem(format!("{at}: removed request body")),
            (None, Some(body)) if body["required"] == json!(true) => {
                self.problem(format!("{at}: new required request body"));
            }
            (Some(old_body), Some(new_body)) => {
                if new_body["required"] == json!(true) && old_body["required"] != json!(true) {
                    self.problem(format!("{at}: request body became required"));
                }
                self.content(&format!("{at} request"), old_body, new_body, Dir::Request);
            }
            _ => {}
        }

        let new_responses = obj(&new_op["responses"]);
        for (code, old_response) in obj(&old_op["responses"]) {
            let Some(new_response) = new_responses.get(&code) else {
                self.problem(format!("{at}: removed response {code}"));
                continue;
            };
            let new_headers = obj(&new_response["headers"]);
            for (name, old_header) in obj(&old_response["headers"]) {
                match new_headers.get(&name) {
                    None => self.problem(format!("{at}: response {code} removed header {name}")),
                    Some(new_header) => self.schema(
                        &format!("{at} response {code} header {name}"),
                        &old_header["schema"],
                        &new_header["schema"],
                        Dir::Response,
                    ),
                }
            }
            self.content(
                &format!("{at} response {code}"),
                &old_response,
                new_response,
                Dir::Response,
            );
        }
    }

    fn content(&mut self, at: &str, old: &Value, new: &Value, dir: Dir) {
        let new_content = obj(&new["content"]);
        for (media, old_media) in obj(&old["content"]) {
            match new_content.get(&media) {
                None => self.problem(format!("{at}: removed media type {media}")),
                Some(new_media) => self.schema(
                    &format!("{at} {media}"),
                    &old_media["schema"],
                    &new_media["schema"],
                    dir,
                ),
            }
        }
    }

    /// Follows a local `$ref`; returns the schema and its component name.
    fn resolve(doc: &Value, schema: &Value) -> (Value, Option<String>) {
        match schema.get("$ref").and_then(Value::as_str) {
            Some(reference) => {
                let name = reference
                    .strip_prefix("#/components/schemas/")
                    .unwrap_or(reference)
                    .to_owned();
                (doc["components"]["schemas"][&name].clone(), Some(name))
            }
            None => (schema.clone(), None),
        }
    }

    fn schema(&mut self, at: &str, old: &Value, new: &Value, dir: Dir) {
        // A false schema accepts no values; true accepts every value. Handle
        // these before object keywords so composite matching cannot mistake
        // an impossible branch for an unconstrained one.
        let old_false = old == &Value::Bool(false);
        let new_false = new == &Value::Bool(false);
        if (dir == Dir::Request && old_false) || (dir == Dir::Response && new_false) {
            return;
        }
        if (dir == Dir::Request && new_false) || (dir == Dir::Response && old_false) {
            self.problem(format!(
                "{at}: boolean schema {} ({})",
                if dir == Dir::Request {
                    "narrowed"
                } else {
                    "widened"
                },
                dir.label()
            ));
            return;
        }
        let unconstrained = json!({});
        let old = if old == &Value::Bool(true) {
            &unconstrained
        } else {
            old
        };
        let new = if new == &Value::Bool(true) {
            &unconstrained
        } else {
            new
        };
        let (old_null, old_core) = split_null(old);
        let (new_null, new_core) = split_null(new);
        let side = dir.label();
        match dir {
            Dir::Request if old_null && !new_null => {
                self.problem(format!("{at}: no longer accepts null ({side})"));
            }
            Dir::Response if new_null && !old_null => {
                self.problem(format!("{at}: may now be null ({side})"));
            }
            _ => {}
        }

        let (old_schema, old_ref) = Self::resolve(self.old, &old_core);
        let (new_schema, new_ref) = Self::resolve(self.new, &new_core);
        if old_ref.is_none() && new_ref.is_none() {
            self.shape(at, &old_core, &new_core, dir);
            return;
        }
        // `$ref` siblings apply at each occurrence, even when the referenced
        // component pair was already compared elsewhere or is recursive.
        let siblings = |schema: &Value, referenced: bool| {
            let mut shape = if referenced { obj(schema) } else { Map::new() };
            shape.remove("$ref");
            Value::Object(shape)
        };
        self.shape(
            at,
            &siblings(&old_core, old_ref.is_some()),
            &siblings(&new_core, new_ref.is_some()),
            dir,
        );
        if let (Some(old_name), Some(new_name)) = (&old_ref, &new_ref)
            && !self.seen.insert((old_name.clone(), new_name.clone(), dir))
        {
            return;
        }
        // The referenced schema may be nullable or a reference itself.
        let at = old_ref.unwrap_or_else(|| at.to_owned());
        self.schema(&at, &old_schema, &new_schema, dir);
    }

    fn shape(&mut self, at: &str, old: &Value, new: &Value, dir: Dir) {
        let side = dir.label();
        // Branch coverage must not assume compatibility for validation keywords
        // the checker does not interpret. Keep unchanged ones valid, and ignore
        // only annotations that cannot constrain the accepted values.
        let unsupported = |schema: &Value| {
            let mut keywords = obj(schema);
            keywords.retain(|key, _| {
                !matches!(
                    key.as_str(),
                    "$ref"
                        | "type"
                        | "format"
                        | "enum"
                        | "default"
                        | "oneOf"
                        | "anyOf"
                        | "allOf"
                        | "properties"
                        | "required"
                        | "additionalProperties"
                        | "items"
                        | "maxLength"
                        | "minLength"
                        | "maxItems"
                        | "minItems"
                        | "maximum"
                        | "minimum"
                        | "exclusiveMaximum"
                        | "exclusiveMinimum"
                        | "pattern"
                        | "multipleOf"
                        | "uniqueItems"
                        | "title"
                        | "description"
                        | "example"
                        | "examples"
                        | "$comment"
                        | "deprecated"
                        | "externalDocs"
                        | "summary"
                )
            });
            keywords
        };
        let old_unsupported = unsupported(old);
        let new_unsupported = unsupported(new);
        let keywords: BTreeSet<_> = old_unsupported
            .keys()
            .chain(new_unsupported.keys())
            .collect();
        for keyword in keywords {
            if old_unsupported.get(keyword) != new_unsupported.get(keyword) {
                self.problem(format!(
                    "{at}: {keyword} changed (unsupported validation keyword, {side})"
                ));
            }
        }
        for key in ["oneOf", "anyOf", "allOf"] {
            match (
                old.get(key).and_then(Value::as_array),
                new.get(key).and_then(Value::as_array),
            ) {
                (None, None) => {}
                (Some(a), Some(b)) => self.composite(at, key, a, b, dir),
                _ => self.problem(format!("{at}: composite {key} changed ({side})")),
            }
        }

        match (types(old), types(new), dir) {
            (Some(old_types), Some(new_types), Dir::Request) if !covers(&new_types, &old_types) => {
                self.problem(format!("{at}: type narrowed ({side})"));
            }
            (None, Some(_), Dir::Request) => {
                self.problem(format!("{at}: type narrowed ({side})"));
            }
            (Some(old_types), Some(new_types), Dir::Response)
                if !covers(&old_types, &new_types) =>
            {
                self.problem(format!("{at}: type widened ({side})"));
            }
            (Some(_), None, Dir::Response) => {
                self.problem(format!("{at}: type widened ({side})"));
            }
            _ => {}
        }

        match (old.get("format"), new.get("format"), dir) {
            (Some(a), Some(b), _) if a != b => {
                self.problem(format!("{at}: format changed ({side})"));
            }
            (None, Some(_), Dir::Request) | (Some(_), None, Dir::Response) => {
                self.problem(format!("{at}: format changed ({side})"));
            }
            _ => {}
        }

        if let Some(old_values) = old.get("enum").and_then(Value::as_array) {
            match new.get("enum").and_then(Value::as_array) {
                Some(new_values) => {
                    for value in old_values {
                        if !new_values.contains(value) {
                            self.problem(format!("{at}: removed enum value {value} ({side})"));
                        }
                    }
                }
                None if dir == Dir::Response => {
                    self.problem(format!("{at}: enum removed ({side})"))
                }
                None => {}
            }
        } else if dir == Dir::Request && new.get("enum").is_some() {
            self.problem(format!("{at}: type narrowed to an enum ({side})"));
        }

        self.bounds(at, old, new, dir);
        if dir == Dir::Request
            && old.get("default").is_some()
            && old.get("default") != new.get("default")
        {
            self.problem(format!("{at}: default changed ({side})"));
        }
        self.constraints(at, old, new, dir);

        let old_props = obj(&old["properties"]);
        let new_props = obj(&new["properties"]);
        for (name, old_prop) in &old_props {
            match new_props.get(name) {
                None => self.problem(format!("{at}.{name}: removed property ({side})")),
                Some(new_prop) => self.schema(&format!("{at}.{name}"), old_prop, new_prop, dir),
            }
        }
        let old_required = names(&old["required"]);
        let new_required = names(&new["required"]);
        match dir {
            Dir::Request => {
                for name in new_required.difference(&old_required) {
                    self.problem(format!("{at}.{name}: became required ({side})"));
                }
                if new.get("additionalProperties") == Some(&json!(false))
                    && old.get("additionalProperties") != Some(&json!(false))
                {
                    self.problem(format!(
                        "{at}: no longer accepts unknown properties ({side})"
                    ));
                }
            }
            Dir::Response => {
                for name in old_required.difference(&new_required) {
                    self.problem(format!("{at}.{name}: no longer always present ({side})"));
                }
            }
        }
        self.nested_schema(
            &format!("{at}.*"),
            old.get("additionalProperties"),
            new.get("additionalProperties"),
            dir,
        );
        self.nested_schema(&format!("{at}[]"), old.get("items"), new.get("items"), dir);
    }

    /// Missing and `true` accept arbitrary nested values. `false` accepts none.
    /// Comparing only object/object pairs would miss constraint introduction or
    /// removal, including when these schemas occur inside composite branches.
    fn nested_schema(&mut self, at: &str, old: Option<&Value>, new: Option<&Value>, dir: Dir) {
        let old_false = old == Some(&Value::Bool(false));
        let new_false = new == Some(&Value::Bool(false));
        if (dir == Dir::Request && old_false) || (dir == Dir::Response && new_false) {
            return;
        }
        let old_shape = old.filter(|value| value.is_object());
        let new_shape = new.filter(|value| value.is_object());
        let broken = match dir {
            Dir::Request => new_false || (old_shape.is_none() && new_shape.is_some()),
            Dir::Response => old_false || (old_shape.is_some() && new_shape.is_none()),
        };
        if broken {
            self.problem(format!(
                "{at}: nested schema {} ({})",
                if dir == Dir::Request {
                    "narrowed"
                } else {
                    "widened"
                },
                dir.label()
            ));
        } else if let (Some(a), Some(b)) = (old_shape, new_shape) {
            self.schema(at, a, b, dir);
        }
    }

    /// Unions must cover each formerly accepted request branch or each newly
    /// emitted response branch. Intersections reverse that requirement because
    /// adding a conjunct narrows the accepted values.
    fn composite(&mut self, at: &str, key: &str, old: &[Value], new: &[Value], dir: Dir) {
        let unchanged = same_branches(old, new);
        if key == "oneOf" && unchanged {
            let old_refs = reachable_components(self.old, &json!(old));
            let new_refs = reachable_components(self.new, &json!(new));
            if old_refs == new_refs
                && old_refs
                    .iter()
                    .all(|reference| self.old.pointer(reference) == self.new.pointer(reference))
            {
                return;
            }
        }
        if key == "oneOf"
            && (!disjoint_branches(self.old, old) || !disjoint_branches(self.new, new))
        {
            self.problem(format!(
                "{at}: composite oneOf changed with potentially overlapping alternatives ({})",
                dir.label()
            ));
            return;
        }
        if unchanged {
            // A referenced branch can change while its `$ref` stays identical.
            for (index, branch) in old.iter().enumerate() {
                self.schema(&format!("{at}.{key}[{index}]"), branch, branch, dir);
            }
            return;
        }
        let mandatory_new = (key == "allOf") == (dir == Dir::Request);
        let (mandatory, candidates) = if mandatory_new {
            (new, old)
        } else {
            (old, new)
        };
        for (index, branch) in mandatory.iter().enumerate() {
            let path = format!("{at}.{key}[{index}]");
            let mut best: Option<Vec<String>> = None;
            for candidate in candidates {
                let (a, b) = if mandatory_new {
                    (candidate, branch)
                } else {
                    (branch, candidate)
                };
                let mut checker = Checker {
                    old: self.old,
                    new: self.new,
                    problems: Vec::new(),
                    seen: self.seen.clone(),
                };
                checker.schema(&path, a, b, dir);
                if best
                    .as_ref()
                    .is_none_or(|best| checker.problems.len() < best.len())
                {
                    best = Some(checker.problems);
                }
                if best.as_ref().is_some_and(Vec::is_empty) {
                    break;
                }
            }
            match best {
                Some(problems) => self.problems.extend(problems),
                None => self.problem(format!(
                    "{path}: no compatible composite branch ({})",
                    dir.label()
                )),
            }
        }
    }

    /// Unknown pattern or multiple changes cannot be proven compatible. Dropping
    /// a request constraint or adding a response constraint is safe.
    fn constraints(&mut self, at: &str, old: &Value, new: &Value, dir: Dir) {
        for key in ["pattern", "multipleOf", "uniqueItems"] {
            let a = old.get(key).filter(|value| **value != json!(false));
            let b = new.get(key).filter(|value| **value != json!(false));
            let broken = match dir {
                Dir::Request => b.is_some() && a != b,
                Dir::Response => a.is_some() && a != b,
            };
            if broken {
                self.problem(format!("{at}: {key} changed ({})", dir.label()));
            }
        }
    }

    /// Requests may not tighten a bound; responses may not loosen one.
    fn bounds(&mut self, at: &str, old: &Value, new: &Value, dir: Dir) {
        let side = dir.label();
        let num = |schema: &Value, key: &str| schema.get(key).and_then(Value::as_f64);
        for (key, upper) in [
            ("maxLength", true),
            ("maxItems", true),
            ("maximum", true),
            ("exclusiveMaximum", true),
            ("minLength", false),
            ("minItems", false),
            ("minimum", false),
            ("exclusiveMinimum", false),
        ] {
            let (a, b) = (num(old, key), num(new, key));
            let tighter = |a: f64, b: f64| if upper { b < a } else { b > a };
            let broken = match dir {
                Dir::Request => match (a, b) {
                    (None, Some(_)) => true,
                    (Some(a), Some(b)) => tighter(a, b),
                    _ => false,
                },
                Dir::Response => match (a, b) {
                    (Some(_), None) => true,
                    (Some(a), Some(b)) => tighter(b, a),
                    _ => false,
                },
            };
            if broken {
                let change = if dir == Dir::Request {
                    "tightened"
                } else {
                    "loosened"
                };
                self.problem(format!("{at}: {key} {change} ({side})"));
            }
        }
    }
}
