//! Composes primary provider credentials with independently managed gateway OAuth credentials.

use std::sync::Arc;

use crate::auth::ResolvedProviderAuth;
use dex_api::AuthError;
use dex_api::AuthHeaderTelemetry;
use dex_api::AuthHeadersFuture;
use dex_api::AuthProvider;
use dex_api::AuthProviderFuture;
use dex_api::SharedAuthProvider;
use dex_login::GatewayAuthError;
use dex_login::GatewayAuthManager;
use dex_model_provider_info::GatewayOAuthConfig;
use dex_model_provider_info::GatewayOAuthDelivery;
use dex_model_provider_info::ModelProviderInfo;
use dex_protocol::error::DexErr;
use dex_protocol::error::DexErrorDetails;
use dex_protocol::error::Result;
use http::HeaderMap;
use http::HeaderName;
use http::HeaderValue;

pub(crate) async fn compose_auth(
    provider: &ModelProviderInfo,
    manager: Option<&std::result::Result<Arc<GatewayAuthManager>, String>>,
    mut resolved: ResolvedProviderAuth,
) -> Result<ResolvedProviderAuth> {
    let Some(config) = provider.gateway_oauth.as_ref() else {
        return Ok(resolved);
    };
    provider.validate().map_err(DexErr::InvalidRequest)?;
    let manager = manager.ok_or_else(|| {
        DexErr::InvalidRequest("gateway_oauth requires auth runtime configuration".into())
    })?;
    let manager = manager
        .as_ref()
        .map_err(|error| DexErr::InvalidRequest(error.clone()))?;
    let token = manager.resolve_access_token().await.map_err(|error| {
        if let Some(login_error) = error
            .get_ref()
            .and_then(|source| source.downcast_ref::<GatewayAuthError>())
        {
            return match login_error {
                GatewayAuthError::LoginRequired => {
                    DexErrorDetails::GatewayLoginRequired(login_error.to_string()).into()
                }
                GatewayAuthError::LoginInProgress => {
                    DexErr::InvalidRequest(login_error.to_string())
                }
            };
        }
        // Issuer errors may echo arbitrary credentials from configured URLs. Keep the
        // diagnostic safe and bounded, preserving retries for transient failures.
        DexErr::Io(std::io::Error::new(
            error.kind(),
            "Gateway OAuth authentication failed; check the gateway configuration and credential store.",
        ))
    })?;
    let (name, value) = gateway_header(config, &token)?;
    resolved.auth = Arc::new(CombinedAuth {
        primary: resolved.auth,
        name,
        value,
    });
    Ok(resolved)
}

fn gateway_header(config: &GatewayOAuthConfig, token: &str) -> Result<(HeaderName, HeaderValue)> {
    if token.is_empty() {
        return Err(DexErr::InvalidRequest(
            "gateway OAuth returned an empty token".into(),
        ));
    }
    let (name, value) = match &config.delivery {
        GatewayOAuthDelivery::Header { name, scheme } => (
            HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| DexErr::InvalidRequest("invalid gateway header".into()))?,
            format!("{scheme} {token}"),
        ),
        GatewayOAuthDelivery::Cookie { name } => {
            // RFC 6265 cookie-octet excludes separators, quotes, whitespace and non-ASCII.
            if !token.bytes().all(
                |byte| matches!(byte, 0x21 | 0x23..=0x2b | 0x2d..=0x3a | 0x3c..=0x5b | 0x5d..=0x7e),
            ) {
                return Err(DexErr::InvalidRequest(
                    "gateway OAuth token is not a valid cookie value".into(),
                ));
            }
            (http::header::COOKIE, format!("{name}={token}"))
        }
    };
    let mut value = HeaderValue::from_str(&value)
        .map_err(|_| DexErr::InvalidRequest("invalid gateway OAuth token header".into()))?;
    value.set_sensitive(true);
    Ok((name, value))
}

struct CombinedAuth {
    primary: SharedAuthProvider,
    name: HeaderName,
    value: HeaderValue,
}

impl CombinedAuth {
    fn insert_gateway_header(&self, headers: &mut HeaderMap) -> std::result::Result<(), AuthError> {
        if headers.contains_key(&self.name) {
            return Err(AuthError::Build(format!(
                "The request already sets the {} header. Choose a different header or cookie in gateway_oauth.delivery for the gateway token.",
                self.name
            )));
        }
        headers.insert(self.name.clone(), self.value.clone());
        Ok(())
    }
}

impl AuthProvider for CombinedAuth {
    fn auth_header_telemetry(&self) -> std::result::Result<AuthHeaderTelemetry, AuthError> {
        self.primary.auth_header_telemetry()
    }

    fn add_auth_headers(&self, headers: &mut HeaderMap) -> std::result::Result<(), AuthError> {
        self.primary.add_auth_headers(headers)?;
        self.insert_gateway_header(headers)
    }

    fn apply_auth_headers(&self, headers: HeaderMap) -> AuthHeadersFuture<'_> {
        Box::pin(async move {
            let mut headers = self.primary.apply_auth_headers(headers).await?;
            self.insert_gateway_header(&mut headers)?;
            Ok(headers)
        })
    }

    fn apply_auth(&self, request: dex_http_client::Request) -> AuthProviderFuture<'_> {
        Box::pin(async move {
            let mut request = self.primary.apply_auth(request).await?;
            self.insert_gateway_header(&mut request.headers)?;
            Ok(request)
        })
    }
}
