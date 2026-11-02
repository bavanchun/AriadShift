//! PDF security tool specifications.
use crate::tools::{
    InputArity, Tier, ToolGroup, ToolRuntime, ToolSpec,
    params::{
        DecryptParams, EncryptParams, PermissionsParams, SanitizeParams, UnlockFormsParams,
        schema_for_params,
    },
};
/// Returns the tool specifications in this group.
#[must_use]
pub fn specs() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            id: "pdf.encrypt",
            version: "1",
            group: ToolGroup::Security,
            summary: "Encrypt a PDF document.",
            inputs: InputArity::One,
            input_formats: &["pdf"],
            output_format: "pdf",
            output_formats: &[],
            tier: Tier::Free,
            runtimes: &[ToolRuntime::Local], // Available in local applications.
            engine: "qpdf",
            params_schema: schema_for_params::<EncryptParams>,
            secret_params: &["user_password", "owner_password"],
        },
        ToolSpec {
            id: "pdf.decrypt",
            version: "1",
            group: ToolGroup::Security,
            summary: "Decrypt a PDF document.",
            inputs: InputArity::One,
            input_formats: &["pdf"],
            output_format: "pdf",
            output_formats: &[],
            tier: Tier::Free,
            runtimes: &[ToolRuntime::Local], // Available in local applications.
            engine: "qpdf",
            params_schema: schema_for_params::<DecryptParams>,
            secret_params: &["password"],
        },
        ToolSpec {
            id: "pdf.permissions",
            version: "1",
            group: ToolGroup::Security,
            summary: "Change PDF permissions.",
            inputs: InputArity::One,
            input_formats: &["pdf"],
            output_format: "pdf",
            output_formats: &[],
            tier: Tier::Free,
            runtimes: &[ToolRuntime::Local], // Available in local applications.
            engine: "qpdf",
            params_schema: schema_for_params::<PermissionsParams>,
            secret_params: &["owner_password", "user_password"],
        },
        ToolSpec {
            id: "pdf.sanitize",
            version: "1",
            group: ToolGroup::Security,
            summary: "Remove selected active content from a PDF document.",
            inputs: InputArity::One,
            input_formats: &["pdf"],
            output_format: "pdf",
            output_formats: &[],
            tier: Tier::Free,
            runtimes: &[ToolRuntime::Wasm, ToolRuntime::Local], // Available in browser and local applications.
            engine: "ariad-pdf",
            params_schema: schema_for_params::<SanitizeParams>,
            secret_params: &[],
        },
        ToolSpec {
            id: "pdf.unlock_forms",
            version: "1",
            group: ToolGroup::Security,
            summary: "Unlock PDF form fields.",
            inputs: InputArity::One,
            input_formats: &["pdf"],
            output_format: "pdf",
            output_formats: &[],
            tier: Tier::Free,
            runtimes: &[ToolRuntime::Wasm, ToolRuntime::Local], // Available in browser and local applications.
            engine: "ariad-pdf",
            params_schema: schema_for_params::<UnlockFormsParams>,
            secret_params: &[],
        },
    ]
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn specifications_are_valid_and_local_or_browser_only() {
        let specs = specs();
        crate::tools::validate_specs(&specs).unwrap();
        assert!(
            specs
                .iter()
                .all(|s| !s.runtimes.contains(&ToolRuntime::Cloud))
        );
        for spec in specs {
            let schema = serde_json::to_value(spec.resolved_params_schema()).unwrap();
            for field in spec.secret_params {
                assert_eq!(schema["properties"][field]["x-ariad-secret"], true);
            }
        }
    }
}

#[cfg(test)]
mod contract_tests {
    use crate::tools::params::*;
    use serde::{Serialize, de::DeserializeOwned};
    use serde_json::{Value, json};

    fn round_trip<T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug>(value: T) {
        assert_eq!(
            serde_json::from_value::<T>(serde_json::to_value(&value).unwrap()).unwrap(),
            value
        );
    }

    #[test]
    fn parameter_edge_cases_and_round_trips() {
        for level in [0, 10] {
            let p: CompressParams = serde_json::from_value(json!({"level":level})).unwrap();
            assert!(p.validate().is_err());
        }
        for target in [0, 1023] {
            let p: CompressParams = serde_json::from_value(json!({"target_bytes":target})).unwrap();
            assert!(p.validate().is_err());
        }
        for value in [
            json!({"level":1,"preset":"ebook"}),
            json!({"jpeg_quality":9}),
        ] {
            let p: CompressParams = serde_json::from_value(value).unwrap();
            assert!(p.validate().is_err());
        }
        let combined = [crate::tools::specs::optimize::specs(), super::specs()].concat();
        crate::tools::validate_specs(&combined).unwrap();
        let compress = CompressParams::default();
        assert_eq!(compress.effective_preset(), Some(CompressPreset::Ebook));
        assert!(serde_json::from_value::<PdfPermissions>(json!({"unknown": true})).is_err());
        assert!(
            serde_json::from_value::<EncryptParams>(
                json!({"owner_password":"x", "algorithm":"rc4"})
            )
            .is_err()
        );
        for algorithm in ["aes-256", "aes-128"] {
            let params: EncryptParams =
                serde_json::from_value(json!({"owner_password":"owner", "algorithm":algorithm}))
                    .unwrap();
            assert_eq!(
                serde_json::to_value(params).unwrap()["algorithm"],
                algorithm
            );
        }
        let encrypt: EncryptParams = serde_json::from_value(json!({"owner_password":""})).unwrap();
        assert!(encrypt.validate().is_err());
        let sanitize: SanitizeParams = serde_json::from_value(json!({})).unwrap();
        assert_eq!(sanitize, SanitizeParams::default());
        assert!(sanitize.javascript && sanitize.embedded_files && sanitize.thumbnails);
        assert!(!sanitize.links && !sanitize.metadata && !sanitize.xmp);
        assert_eq!(sanitize.forms, SanitizeForms::Keep);
        let invalid: SanitizeParams =
            serde_json::from_value(json!({"annotations_of_type":["/Link"]})).unwrap();
        assert!(invalid.validate().is_err());
        let schema =
            serde_json::to_value(crate::tools::params::schema_for_params::<SanitizeParams>())
                .unwrap();
        assert!(!jsonschema::is_valid(
            &schema,
            &json!({"annotations_of_type":["/Link"]})
        ));
        let decrypt: DecryptParams = serde_json::from_value(json!({})).unwrap();
        assert_eq!(decrypt.password, None);
        let permissions: PermissionsParams =
            serde_json::from_value(json!({"owner_password":"owner"})).unwrap();
        assert!(serde_json::from_value::<PermissionsParams>(json!({"owner_password":""})).is_ok());
        round_trip(compress);
        round_trip(GrayscaleParams::default());
        round_trip(encrypt);
        round_trip(decrypt);
        round_trip(permissions);
        round_trip(RepairParams::default());
        round_trip(LinearizeParams::default());
        round_trip(sanitize);
        round_trip(UnlockFormsParams::default());
        for schema in [crate::tools::specs::optimize::specs(), super::specs()].concat() {
            let raw: Value = json!({"unexpected":true});
            assert!(!jsonschema::is_valid(
                &serde_json::to_value(schema.resolved_params_schema()).unwrap(),
                &raw
            ));
        }
    }
}

#[cfg(test)]
mod secret_debug_tests {
    use crate::tools::params::{DecryptParams, EncryptParams, PermissionsParams};
    use serde_json::json;
    #[test]
    fn password_parameter_debug_output_is_redacted() {
        let value = "sensitive-debug-value";
        let decrypt: DecryptParams = serde_json::from_value(json!({"password": value})).unwrap();
        let encrypt: EncryptParams =
            serde_json::from_value(json!({"owner_password": value, "user_password": value}))
                .unwrap();
        let permissions: PermissionsParams =
            serde_json::from_value(json!({"owner_password": value, "user_password": value}))
                .unwrap();
        for rendered in [
            format!("{decrypt:?}"),
            format!("{encrypt:?}"),
            format!("{permissions:?}"),
        ] {
            assert!(!rendered.contains(value));
            assert!(rendered.contains("[REDACTED]"));
        }
    }
}
