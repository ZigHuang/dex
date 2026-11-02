//! ChatGPT eligibility, credentials, and account identity for managed configuration.

use crate::backend::BundleClient;
use crate::backend::BundleRequestError;
use crate::backend::RetryableFailureKind;
use crate::config_source::BundleSource;
use dex_config::CloudConfigBundle;
use dex_config::CloudConfigBundleLoadError;
use dex_login::AuthManager;
use dex_login::DexAuth;
use dex_login::RefreshTokenError;
use dex_login::UnauthorizedRecovery;
use dex_protocol::account::PlanType;
use std::sync::Arc;

const CLOUD_CONFIG_BUNDLE_AUTH_RECOVERY_FAILED_MESSAGE: &str = concat!(
    "Your authentication session could not be refreshed automatically. ",
    "Please log out and sign in again."
);

pub(crate) struct ChatGptSource<C> {
    pub(crate) auth_manager: Arc<AuthManager>,
    pub(crate) client: Arc<C>,
}

impl<C: BundleClient> BundleSource for ChatGptSource<C> {
    type Auth = DexAuth;
    type FetchContext = UnauthorizedRecovery;
    fn begin_fetch(&self) -> Self::FetchContext {
        self.auth_manager.unauthorized_recovery()
    }

    async fn auth(&self) -> Result<Option<DexAuth>, CloudConfigBundleLoadError> {
        Ok(self.auth_manager.auth().await.filter(|auth| {
            auth.uses_dex_backend()
                && auth.account_plan_type().is_some_and(|plan| {
                    plan.is_business_like()
                        || plan.is_education_like()
                        || matches!(
                            plan,
                            PlanType::Enterprise | PlanType::Sci | PlanType::SciPlus
                        )
                })
        }))
    }

    fn identity(&self, auth: &DexAuth) -> (Option<String>, Option<String>) {
        (auth.get_chatgpt_user_id(), auth.get_account_id())
    }

    async fn get_bundle(
        &self,
        auth: &mut DexAuth,
        _context: &mut UnauthorizedRecovery,
    ) -> Result<CloudConfigBundle, BundleRequestError> {
        self.client.get_bundle(auth).await
    }

    async fn recover_auth(
        &self,
        auth: &mut DexAuth,
        recovery: &mut UnauthorizedRecovery,
        status_code: Option<u16>,
        _recovery_attempt: usize,
    ) -> Result<bool, BundleRequestError> {
        if !recovery.has_next() {
            return Err(BundleRequestError::auth_failure(
                status_code,
                CLOUD_CONFIG_BUNDLE_AUTH_RECOVERY_FAILED_MESSAGE,
                Some("auth_recovery_unavailable"),
            ));
        }
        match recovery.next().await {
            Ok(_) => {
                *auth = self.auth_manager.auth().await.ok_or_else(|| {
                    BundleRequestError::auth_failure(
                        status_code,
                        CLOUD_CONFIG_BUNDLE_AUTH_RECOVERY_FAILED_MESSAGE,
                        Some("auth_recovery_missing_auth"),
                    )
                })?;
                Ok(true)
            }
            Err(RefreshTokenError::Transient(_)) => Err(BundleRequestError::Retryable(
                RetryableFailureKind::AuthRecovery { status_code },
            )),
            Err(RefreshTokenError::Permanent(failed)) => Err(BundleRequestError::auth_failure(
                status_code,
                failed.message,
                Some("auth_recovery_unrecoverable"),
            )),
            Err(RefreshTokenError::Policy(error)) => Err(BundleRequestError::auth_failure(
                status_code,
                error.to_string(),
                /*metric_reason*/ None,
            )),
        }
    }
}
