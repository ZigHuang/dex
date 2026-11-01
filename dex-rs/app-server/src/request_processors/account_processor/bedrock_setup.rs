use super::super::bedrock_auth::ensure_user_model_provider_can_be_bedrock;
use super::super::config_processor::map_error as map_config_error;
use super::AccountRequestProcessor;
use crate::error_code::internal_error;
use crate::error_code::invalid_request;
use dex_app_server_protocol::AwsCredentialType;
use dex_app_server_protocol::BedrockAwsProfile;
use dex_app_server_protocol::BedrockDiscoverParams;
use dex_app_server_protocol::BedrockDiscoverResponse;
use dex_app_server_protocol::BedrockEnvironmentCredential;
use dex_app_server_protocol::BedrockSetupParams;
use dex_app_server_protocol::BedrockSetupResponse;
use dex_app_server_protocol::ClientResponsePayload;
use dex_app_server_protocol::ConfigBatchWriteParams;
use dex_app_server_protocol::ConfigEdit;
use dex_app_server_protocol::JSONRPCErrorError;
use dex_app_server_protocol::MergeStrategy;
use dex_login::DexAuth;
use dex_model_provider::AMAZON_BEDROCK_PROVIDER_ID;
use dex_model_provider::is_supported_amazon_bedrock_region;

const AWS_ACCESS_KEY_ID: &str = "AWS_ACCESS_KEY_ID";
const AWS_SECRET_ACCESS_KEY: &str = "AWS_SECRET_ACCESS_KEY";
const AWS_BEARER_TOKEN_BEDROCK: &str = "AWS_BEARER_TOKEN_BEDROCK";

impl AccountRequestProcessor {
    pub(crate) async fn bedrock_discover(
        &self,
        _params: BedrockDiscoverParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        self.ensure_bedrock_login_allowed()?;
        let profiles = dex_aws_auth::discover_aws_profiles()
            .await
            .map_err(|err| internal_error(format!("failed to discover AWS profiles: {err}")))?
            .into_iter()
            .map(|profile| BedrockAwsProfile {
                name: profile.name,
                region: profile.region,
            })
            .collect();

        let region = non_empty_env_var("AWS_REGION");
        let environment_credentials = [
            (
                AwsCredentialType::AccessKeys,
                non_empty_env_var(AWS_ACCESS_KEY_ID).is_some()
                    && non_empty_env_var(AWS_SECRET_ACCESS_KEY).is_some(),
            ),
            (
                AwsCredentialType::BedrockApiKey,
                non_empty_env_var(AWS_BEARER_TOKEN_BEDROCK).is_some(),
            ),
        ]
        .into_iter()
        .filter(|(_, available)| *available)
        .map(|(credential_type, _)| BedrockEnvironmentCredential {
            credential_type,
            region: region.clone(),
        })
        .collect();

        Ok(Some(
            BedrockDiscoverResponse {
                profiles,
                environment_credentials,
            }
            .into(),
        ))
    }

    pub(crate) async fn bedrock_setup(
        &self,
        params: BedrockSetupParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        self.ensure_bedrock_login_allowed()?;
        ensure_user_model_provider_can_be_bedrock(&self.config_manager).await?;
        if matches!(&params, BedrockSetupParams::Environment { .. })
            && matches!(
                self.auth_manager.auth_cached(),
                Some(DexAuth::BedrockApiKey(_))
            )
        {
            return Err(invalid_request(
                "A Bedrock API key is already configured and takes priority over AWS credentials. Run `dex logout` and try again.",
            ));
        }

        let region = match &params {
            BedrockSetupParams::Profile { region, .. }
            | BedrockSetupParams::Environment { region, .. }
            | BedrockSetupParams::AccessKeys { region, .. } => region.trim(),
        };
        if !is_supported_amazon_bedrock_region(region) {
            return Err(invalid_request(format!(
                "Amazon Bedrock Mantle does not support region `{region}`"
            )));
        }

        let profile = match &params {
            BedrockSetupParams::Profile { profile, .. } => {
                let profile = profile.trim();
                if profile.is_empty() {
                    return Err(invalid_request("AWS profile name must not be empty."));
                }
                dex_aws_auth::validate_aws_profile(profile, region)
                    .await
                    .map_err(|err| {
                        invalid_request(format!(
                            "failed to load credentials for AWS profile `{profile}`: {err}"
                        ))
                    })?;
                Some(profile.to_string())
            }
            BedrockSetupParams::Environment { .. } => {
                let has_bedrock_api_key = non_empty_env_var(AWS_BEARER_TOKEN_BEDROCK).is_some();
                let has_access_keys = non_empty_env_var(AWS_ACCESS_KEY_ID).is_some()
                    && non_empty_env_var(AWS_SECRET_ACCESS_KEY).is_some();
                if !has_bedrock_api_key && !has_access_keys {
                    return Err(invalid_request(
                        "No AWS credentials found. Please Configure AWS credentials or complete AWS sign-in, then try again.",
                    ));
                }
                None
            }
            BedrockSetupParams::AccessKeys { .. } => {
                // TODO: Support direct AWS access key setup with Dex-managed credential storage.
                return Err(invalid_request(
                    "Direct AWS access key setup is not supported yet.",
                ));
            }
        };

        // TODO: Validate the selected credentials against Bedrock ListModels once supported.
        self.cancel_active_login().await;
        self.config_manager
            .batch_write(ConfigBatchWriteParams {
                edits: [
                    (
                        "model_provider",
                        serde_json::json!(AMAZON_BEDROCK_PROVIDER_ID),
                    ),
                    (
                        "model_providers.amazon-bedrock.aws.region",
                        serde_json::json!(region),
                    ),
                    (
                        "model_providers.amazon-bedrock.aws.profile",
                        serde_json::json!(profile),
                    ),
                ]
                .into_iter()
                .map(|(key_path, value)| ConfigEdit {
                    key_path: key_path.to_string(),
                    value,
                    merge_strategy: MergeStrategy::Replace,
                })
                .collect(),
                file_path: None,
                expected_version: None,
                reload_user_config: false,
            })
            .await
            .map_err(map_config_error)?;

        Ok(Some(BedrockSetupResponse {}.into()))
    }
}

fn non_empty_env_var(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}
