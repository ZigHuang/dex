use std::sync::Arc;
use std::sync::RwLock;

use dex_app_server_protocol::ChatgptAuthTokensRefreshParams;
use dex_app_server_protocol::ChatgptAuthTokensRefreshReason;
use dex_app_server_protocol::ChatgptAuthTokensRefreshResponse;
use dex_app_server_protocol::ServerRequestPayload;
use dex_login::DexAuth;
use dex_login::ExternalAuthFuture;
use dex_login::auth::ExternalAuth;
use dex_login::auth::ExternalAuthRefreshContext;
use dex_login::auth::ExternalAuthRefreshReason;
use tokio::time::Duration;
use tokio::time::timeout;

use crate::outgoing_message::OutgoingMessageSender;

const EXTERNAL_AUTH_REFRESH_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) struct ExternalAuthBridge {
    outgoing: Arc<OutgoingMessageSender>,
    auth: RwLock<DexAuth>,
}

impl ExternalAuthBridge {
    pub(crate) fn new(outgoing: Arc<OutgoingMessageSender>, auth: DexAuth) -> Self {
        Self {
            outgoing,
            auth: RwLock::new(auth),
        }
    }

    async fn refresh(&self, context: ExternalAuthRefreshContext) -> std::io::Result<DexAuth> {
        let reason = match context.reason {
            ExternalAuthRefreshReason::Unauthorized => ChatgptAuthTokensRefreshReason::Unauthorized,
        };
        let params = ChatgptAuthTokensRefreshParams {
            reason,
            previous_account_id: context.previous_account_id,
        };

        let (request_id, rx) = self
            .outgoing
            .send_request(ServerRequestPayload::ChatgptAuthTokensRefresh(params))
            .await;
        let result = match timeout(EXTERNAL_AUTH_REFRESH_TIMEOUT, rx).await {
            Ok(result) => {
                let result = result.map_err(|err| {
                    std::io::Error::other(format!("auth refresh request canceled: {err}"))
                })?;
                result.map_err(|err| {
                    // Don't log err.message because it may contain a token.
                    let code = err.code;
                    std::io::Error::other(format!("auth refresh request failed: code={code}"))
                })?
            }
            Err(_) => {
                let _canceled = self.outgoing.cancel_request(&request_id).await;
                return Err(std::io::Error::other(format!(
                    "auth refresh request timed out after {}s",
                    EXTERNAL_AUTH_REFRESH_TIMEOUT.as_secs()
                )));
            }
        };

        // Don't propagate parser error messages because they may contain a token.
        let response: ChatgptAuthTokensRefreshResponse = serde_json::from_value(result)
            .map_err(|_| std::io::Error::other("invalid auth refresh response"))?;
        let auth = DexAuth::from_external_chatgpt_tokens(
            response.access_token.as_str(),
            response.chatgpt_account_id.as_str(),
            response.chatgpt_plan_type.as_deref(),
        )
        .map_err(|err| {
            std::io::Error::new(err.kind(), "auth refresh returned invalid credentials")
        })?;
        *self
            .auth
            .write()
            .map_err(|_| std::io::Error::other("external auth lock is poisoned"))? = auth.clone();
        Ok(auth)
    }
}

impl ExternalAuth for ExternalAuthBridge {
    fn resolve(&self) -> ExternalAuthFuture<'_, DexAuth> {
        Box::pin(async {
            self.auth
                .read()
                .map(|auth| auth.clone())
                .map_err(|_| std::io::Error::other("external auth lock is poisoned"))
        })
    }

    fn refresh(&self, context: ExternalAuthRefreshContext) -> ExternalAuthFuture<'_, DexAuth> {
        Box::pin(ExternalAuthBridge::refresh(self, context))
    }
}
