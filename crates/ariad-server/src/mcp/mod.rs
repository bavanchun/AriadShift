//! Authenticated, stateless MCP over Streamable HTTP.

mod tools;

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{FromRequestParts, Request, State},
    http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::never::NeverSessionManager,
};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use tower_http::{
    cors::{Any, CorsLayer},
    timeout::TimeoutLayer,
};
use url::{Position, Url};

use crate::{
    api::{AppState, Caller, Inner, JSON_BODY_LIMIT},
    auth::{Authenticator, Credential},
    config::{ApiConfig, AuthConfig},
    error::ApiError,
};

/// Maximum decoded UTF-8 content accepted by `convert`.
pub const INLINE_CONTENT_LIMIT: usize = 256 * 1024;

/// RFC 9728 metadata for the `/mcp` resource (path-inserted form).
const METADATA_PATH: &str = "/.well-known/oauth-protected-resource/mcp";

/// Root alias of the same document, for clients that look there first.
const METADATA_ROOT_PATH: &str = "/.well-known/oauth-protected-resource";

#[derive(Clone)]
struct Gate {
    state: AppState,
    resource: Url,
    metadata_url: String,
    origins: Vec<String>,
}

/// Mounts `/mcp` and RFC 9728 metadata. A configured public URL is necessary
/// for an unambiguous OAuth audience; without it remote MCP is disabled.
pub fn router(state: AppState, config: &ApiConfig, shutdown: CancellationToken) -> Router {
    let Some(base) = &config.public_url else {
        tracing::warn!("remote MCP disabled: set ARIAD_PUBLIC_URL");
        return Router::new();
    };
    let resource = resource_url(base);
    let metadata_url = format!("{}{METADATA_PATH}", resource.origin().ascii_serialization());
    // Keep REST and MCP audiences separate while sharing their database,
    // quotas, queues, storage, metrics, and workspace identity.
    let mcp_state = AppState::new(Inner {
        db: state.db.clone(),
        storage: state.storage.clone(),
        queue: state.queue.clone(),
        auth: Authenticator::new(
            mcp_auth_config(&config.auth, &resource),
            config.secret.clone(),
            config.trusted_client_ip_header.clone(),
            crate::http_client(),
        ),
        events: state.events.clone(),
        packs: state.packs.clone(),
        metrics: state.metrics.clone(),
    });
    let origins: Vec<String> = config
        .cors_origins
        .iter()
        .filter_map(|raw| origin(raw))
        .collect();
    let gate = Gate {
        state: mcp_state.clone(),
        resource: resource.clone(),
        metadata_url,
        origins: origins.clone(),
    };
    let transport = StreamableHttpServerConfig::default()
        .with_legacy_session_mode(false)
        .with_json_response(true)
        .with_stateless_protocol_metadata_required(true)
        .with_max_request_body_bytes(JSON_BODY_LIMIT)
        .with_allowed_hosts([resource[Position::BeforeHost..Position::AfterPort].to_owned()])
        .enforce_origin_validation()
        .with_cancellation_token(shutdown);
    // Origin checks below use exact scheme/host/effective-port matching. The
    // SDK's portless-origin compatibility rule would allow any port.
    let transport = transport.with_allowed_origins(origins.iter().filter_map(|raw| {
        let url = Url::parse(raw).ok()?;
        Some(format!(
            "{}://{}:{}",
            url.scheme(),
            &url[Position::BeforeHost..Position::AfterHost],
            url.port_or_known_default()?
        ))
    }));
    let service = StreamableHttpService::new(
        move || Ok(tools::RemoteServer::new(mcp_state.clone())),
        Arc::new(NeverSessionManager::default()),
        transport,
    );
    let endpoint = Router::new()
        .route_service("/mcp", service)
        .layer(middleware::from_fn_with_state(gate.clone(), authorize))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            config.request_timeout,
        ));
    let allowed: Vec<HeaderValue> = origins.iter().filter_map(|o| o.parse().ok()).collect();
    let cors = CorsLayer::new()
        .allow_origin(allowed)
        .allow_methods([Method::POST, Method::GET, Method::DELETE])
        .allow_headers([
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            header::ACCEPT,
            HeaderName::from_static(crate::auth::API_KEY_HEADER),
            HeaderName::from_static("mcp-protocol-version"),
            HeaderName::from_static("mcp-method"),
            HeaderName::from_static("mcp-name"),
        ])
        .expose_headers([header::WWW_AUTHENTICATE, header::RETRY_AFTER]);
    let endpoint = endpoint
        .layer(cors)
        .layer(middleware::from_fn_with_state(gate, validate_headers));
    // Discovery metadata is public and read-only, so browser-based clients
    // on any origin may fetch it. The Host and Origin checks guard `/mcp`
    // itself against DNS rebinding and stay on that route.
    let document = Json(protected_resource_metadata(&resource, &config.auth));
    let serve_metadata = move || std::future::ready(document.clone());
    let metadata = Router::new()
        .route(METADATA_PATH, get(serve_metadata.clone()))
        .route(METADATA_ROOT_PATH, get(serve_metadata))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods([Method::GET]),
        );
    endpoint
        .merge(metadata)
        .layer(middleware::from_fn(no_store))
}

/// The MCP resource identifier: the public origin with the path `/mcp`.
/// The auth service derives the same value from the same ARIAD_PUBLIC_URL
/// and issues access tokens with it as their audience (RFC 8707).
fn resource_url(base: &Url) -> Url {
    let mut resource = base.clone();
    resource.set_path("/mcp");
    resource.set_query(None);
    resource.set_fragment(None);
    resource
}

/// RFC 9728 metadata naming the authorization server that issues `/mcp`
/// tokens. Without a configured issuer only API keys work, so none is named.
fn protected_resource_metadata(resource: &Url, auth: &AuthConfig) -> Value {
    json!({
        "resource": resource.as_str(),
        "resource_name": "AriadShift remote MCP",
        "authorization_servers": auth.jwt.as_ref()
            .map(|jwt| vec![jwt.issuer.clone()]).unwrap_or_default(),
        "bearer_methods_supported": ["header"]
    })
}

/// Credentials for `/mcp`: an API key, or a Bearer token from the REST
/// issuer and JWKS whose audience is the MCP resource. The REST audience
/// (the auth origin) is not accepted, and nor are anonymous callers.
fn mcp_auth_config(auth: &AuthConfig, resource: &Url) -> AuthConfig {
    let mut config = auth.clone();
    config.anonymous = false;
    if let Some(jwt) = &mut config.jwt {
        jwt.audience = resource.to_string();
    }
    config
}

fn origin(raw: &str) -> Option<String> {
    let url = Url::parse(raw).ok()?;
    (matches!(url.scheme(), "http" | "https")
        && url.username().is_empty()
        && url.password().is_none()
        && url.path() == "/"
        && url.query().is_none()
        && url.fragment().is_none())
    .then(|| url.origin().ascii_serialization())
}

fn valid_endpoint_headers(headers: &HeaderMap, gate: &Gate) -> bool {
    let hosts = headers.get_all(header::HOST);
    if hosts.iter().count() != 1
        || hosts.iter().next().and_then(|v| v.to_str().ok())
            != Some(&gate.resource[Position::BeforeHost..Position::AfterPort])
    {
        return false;
    }
    let origins = headers.get_all(header::ORIGIN);
    if origins.iter().count() > 1 {
        return false;
    }
    match origins.iter().next() {
        None => true,
        Some(value) => value
            .to_str()
            .ok()
            .and_then(origin)
            .is_some_and(|o| gate.origins.contains(&o)),
    }
}

async fn validate_headers(State(gate): State<Gate>, request: Request, next: Next) -> Response {
    if !valid_endpoint_headers(request.headers(), &gate) {
        return ApiError::forbidden("invalid_origin", "Host or Origin is not allowed")
            .into_response();
    }
    if request
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok()?.parse::<usize>().ok())
        .is_some_and(|length| length > JSON_BODY_LIMIT)
    {
        return ApiError::too_large("MCP request exceeds the body limit").into_response();
    }
    // The SDK enforces the same limit while reading chunked bodies. An outer
    // Limited body would turn its overflow into an SDK transport error (500).
    next.run(request).await
}

async fn authorize(State(gate): State<Gate>, request: Request, next: Next) -> Response {
    let (mut parts, body) = request.into_parts();
    // Caller performs the same failed-credential throttle, entitlement lookup,
    // and per-workspace rate limit as /v1, even for discovery and tool listing.
    let caller = match Caller::from_request_parts(&mut parts, &gate.state).await {
        Ok(caller) if caller.principal.credential != Credential::Anonymous => caller,
        Ok(_) => return challenge(&gate),
        Err(err) if err.status() == StatusCode::UNAUTHORIZED => return challenge(&gate),
        Err(err) => return err.into_response(),
    };
    parts.extensions.insert(Arc::new(caller));
    // Handlers receive only the resolved identity. Credentials are not needed
    // downstream and must never be forwarded to an upstream service.
    parts.headers.remove(header::AUTHORIZATION);
    parts.headers.remove(crate::auth::API_KEY_HEADER);
    next.run(Request::from_parts(parts, body)).await
}

fn challenge(gate: &Gate) -> Response {
    let mut response =
        ApiError::unauthorized("use an MCP-audience access token or x-api-key").into_response();
    if let Ok(value) = HeaderValue::from_str(&format!(
        "Bearer resource_metadata=\"{}\"",
        gate.metadata_url
    )) {
        response
            .headers_mut()
            .insert(header::WWW_AUTHENTICATE, value);
    }
    response
}

async fn no_store(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// Decode bounded, internal handler responses without making loopback HTTP
/// calls or passing the caller's token to the REST surface.
async fn response_json(response: Response) -> Result<Value, ApiError> {
    let bytes = axum::body::to_bytes(response.into_body(), JSON_BODY_LIMIT)
        .await
        .map_err(|err| ApiError::internal(&err))?;
    serde_json::from_slice(&bytes).map_err(|err| ApiError::internal(&err))
}

#[cfg(test)]
mod tests {
    use jsonwebtoken::{
        Algorithm, EncodingKey, Header, encode,
        jwk::{Jwk, JwkSet},
    };

    use super::*;
    use crate::{
        auth::jwt::{JwtError, JwtVerifier},
        config::JwtConfig,
    };

    const AUTH: &str = "https://auth.example.com";

    fn auth_config(jwt: bool) -> AuthConfig {
        AuthConfig {
            jwt: jwt.then(|| JwtConfig {
                jwks_url: "http://auth:3000/api/auth/jwks".parse().unwrap(),
                issuer: AUTH.to_owned(),
                audience: AUTH.to_owned(),
            }),
            api_keys: true,
            api_key_config_ids: vec!["default".to_owned()],
            anonymous: true,
            turnstile: None,
        }
    }

    fn resource() -> Url {
        resource_url(&"https://api.example.com".parse().unwrap())
    }

    #[test]
    fn the_resource_is_the_public_origin_with_the_mcp_path() {
        for (base, expected) in [
            ("https://api.example.com", "https://api.example.com/mcp"),
            ("https://api.example.com/", "https://api.example.com/mcp"),
            (
                "https://api.example.com/v1?x=1#y",
                "https://api.example.com/mcp",
            ),
            ("http://localhost:8080", "http://localhost:8080/mcp"),
            ("https://api.example.com:443", "https://api.example.com/mcp"),
        ] {
            assert_eq!(
                resource_url(&base.parse().unwrap()).as_str(),
                expected,
                "{base}"
            );
        }
    }

    #[test]
    fn metadata_names_the_resource_and_its_authorization_server() {
        assert_eq!(
            protected_resource_metadata(&resource(), &auth_config(true)),
            json!({
                "resource": "https://api.example.com/mcp",
                "resource_name": "AriadShift remote MCP",
                "authorization_servers": [AUTH],
                "bearer_methods_supported": ["header"]
            })
        );
        let keys_only = protected_resource_metadata(&resource(), &auth_config(false));
        assert_eq!(keys_only["authorization_servers"], json!([]));
        assert_eq!(METADATA_PATH, "/.well-known/oauth-protected-resource/mcp");
        assert_eq!(METADATA_ROOT_PATH, "/.well-known/oauth-protected-resource");
    }

    #[test]
    fn mcp_credentials_require_the_mcp_audience_and_no_anonymous_access() {
        let config = mcp_auth_config(&auth_config(true), &resource());
        assert!(!config.anonymous);
        assert!(config.api_keys);
        let jwt = config.jwt.expect("jwt");
        assert_eq!(jwt.audience, "https://api.example.com/mcp");
        assert_eq!(jwt.issuer, AUTH);
        assert_eq!(jwt.jwks_url.as_str(), "http://auth:3000/api/auth/jwks");
        assert!(
            mcp_auth_config(&auth_config(false), &resource())
                .jwt
                .is_none()
        );
    }

    /// Signs tokens with an RFC 8410 test seed, never a service key, and
    /// verifies them with the MCP verifier built from the REST settings.
    struct Issuer {
        key: EncodingKey,
        verifier: JwtVerifier,
    }

    impl Issuer {
        fn new() -> Self {
            let mut der = hex::decode("302e020100300506032b657004220420").unwrap();
            der.extend_from_slice(&[9; 32]);
            let key = EncodingKey::from_ed_der(&der);
            let mut jwk = Jwk::from_encoding_key(&key, Algorithm::EdDSA).unwrap();
            jwk.common.key_id = Some("k1".to_owned());
            let config = mcp_auth_config(&auth_config(true), &resource())
                .jwt
                .expect("jwt");
            let verifier = JwtVerifier::with_keys(config, JwkSet { keys: vec![jwk] });
            Self { key, verifier }
        }

        fn sign(&self, claims: &Value) -> String {
            let mut header = Header::new(Algorithm::EdDSA);
            header.kid = Some("k1".to_owned());
            header.typ = Some("at+jwt".to_owned());
            encode(&header, claims, &self.key).unwrap()
        }

        async fn verify(&self, claims: Value) -> Result<String, JwtError> {
            self.verifier.verify(&self.sign(&claims)).await
        }
    }

    fn claims(aud: Value, iss: &str, exp: i64) -> Value {
        json!({"sub": "alice", "iss": iss, "aud": aud, "exp": exp, "client_id": "c"})
    }

    const LATER: i64 = 4_102_444_800;

    #[tokio::test]
    async fn oauth_access_tokens_for_the_mcp_resource_are_accepted() {
        let issuer = Issuer::new();
        let mcp = "https://api.example.com/mcp";
        assert_eq!(
            issuer
                .verify(claims(json!(mcp), AUTH, LATER))
                .await
                .unwrap(),
            "alice"
        );
        // Better Auth adds its UserInfo endpoint when `openid` is granted.
        let with_userinfo = json!([mcp, "https://auth.example.com/api/auth/oauth2/userinfo"]);
        assert_eq!(
            issuer
                .verify(claims(with_userinfo, AUTH, LATER))
                .await
                .unwrap(),
            "alice"
        );
    }

    #[tokio::test]
    async fn other_audiences_issuers_and_expired_tokens_are_rejected() {
        let issuer = Issuer::new();
        let mcp = "https://api.example.com/mcp";
        for (name, token_claims) in [
            // The ordinary API JWT from /api/auth/token.
            ("REST audience", claims(json!(AUTH), AUTH, LATER)),
            (
                "another resource",
                claims(json!("https://api.example.com/v1"), AUTH, LATER),
            ),
            (
                "resource prefix",
                claims(json!("https://api.example.com/mcp/x"), AUTH, LATER),
            ),
            (
                "foreign issuer",
                claims(json!(mcp), "https://evil.example.com", LATER),
            ),
            ("expired", claims(json!(mcp), AUTH, 1_000_000_000)),
            (
                "no audience",
                json!({"sub": "alice", "iss": AUTH, "exp": LATER}),
            ),
        ] {
            assert!(
                matches!(issuer.verify(token_claims).await, Err(JwtError::Invalid(_))),
                "{name}"
            );
        }
    }
}
