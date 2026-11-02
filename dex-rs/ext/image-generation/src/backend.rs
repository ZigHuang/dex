use dex_api::ImageEditRequest;
use dex_api::ImageGenerationRequest;
use dex_api::ImageRequestError;
use dex_api::ImageResponse;
use dex_api::ImagesClient;
use dex_api::ReqwestTransport;
use dex_api::map_api_error;
use dex_core::X_DEX_TURN_METADATA_HEADER;
use dex_http_client::ClientRouteClass;
use dex_http_client::HttpClientFactory;
use dex_login::default_client::add_originator_header;
use dex_login::default_client::create_transport_for_routes_async;
use dex_model_provider::SharedModelProvider;
use dex_protocol::error::DexErr;
use http::HeaderMap;
use http::HeaderValue;

const X_DEX_IMAGE_TURN_ID_HEADER: &str = "x-dex-image-turn-id";

pub(crate) struct ImageBackendError {
    message: String,
    dex_error: DexErr,
    imagegen_request_id: Option<String>,
}

impl ImageBackendError {
    fn from_image_request(error: ImageRequestError) -> Self {
        let (error, imagegen_request_id) = error.into_parts();
        let message = error.to_string();
        Self {
            message,
            dex_error: map_api_error(error),
            imagegen_request_id,
        }
    }

    fn from_message(message: String) -> Self {
        Self {
            dex_error: DexErr::Stream(message.clone()),
            message,
            imagegen_request_id: None,
        }
    }

    pub(crate) fn message(&self) -> &str {
        &self.message
    }

    pub(crate) fn dex_error(&self) -> &DexErr {
        &self.dex_error
    }

    pub(crate) fn imagegen_request_id(&self) -> Option<&str> {
        self.imagegen_request_id.as_deref()
    }
}

#[derive(Clone)]
pub(crate) struct DexImagesBackend {
    provider: SharedModelProvider,
    http_client_factory: HttpClientFactory,
    originator: Option<String>,
}

impl DexImagesBackend {
    /// Creates a backend that sends image requests through the active model provider.
    pub(crate) fn new(
        provider: SharedModelProvider,
        http_client_factory: HttpClientFactory,
        originator: Option<String>,
    ) -> Self {
        Self {
            provider,
            http_client_factory,
            originator,
        }
    }

    /// Resolves the provider and auth required for the current image API request.
    async fn client(&self) -> Result<ImagesClient<ReqwestTransport>, ImageBackendError> {
        let (provider, auth) = self
            .provider
            .api_provider_and_auth()
            .await
            .map_err(|err| ImageBackendError::from_message(err.to_string()))?;
        let transport = create_transport_for_routes_async(
            self.http_client_factory.clone(),
            ClientRouteClass::Api,
        )
        .await
        .map_err(|err| ImageBackendError::from_message(err.to_string()))?;
        Ok(ImagesClient::new(transport, provider, auth))
    }

    /// Sends a standalone image generation request through the configured Images client.
    pub(crate) async fn generate(
        &self,
        request: ImageGenerationRequest,
        turn_id: &str,
        dex_turn_metadata: Option<&str>,
    ) -> Result<(ImageResponse, Option<String>), ImageBackendError> {
        self.client()
            .await?
            .generate(
                &request,
                image_request_headers(self.originator.as_deref(), turn_id, dex_turn_metadata),
            )
            .await
            .map_err(ImageBackendError::from_image_request)
    }

    /// Sends a standalone image edit request through the configured Images client.
    pub(crate) async fn edit(
        &self,
        request: ImageEditRequest,
        turn_id: &str,
        dex_turn_metadata: Option<&str>,
    ) -> Result<(ImageResponse, Option<String>), ImageBackendError> {
        self.client()
            .await?
            .edit(
                &request,
                image_request_headers(self.originator.as_deref(), turn_id, dex_turn_metadata),
            )
            .await
            .map_err(ImageBackendError::from_image_request)
    }
}

fn image_request_headers(
    originator: Option<&str>,
    turn_id: &str,
    dex_turn_metadata: Option<&str>,
) -> HeaderMap {
    let mut headers = HeaderMap::new();
    if let Ok(turn_id) = HeaderValue::from_str(turn_id) {
        headers.insert(X_DEX_IMAGE_TURN_ID_HEADER, turn_id);
    }
    if let Some(originator) = originator {
        add_originator_header(&mut headers, originator);
    }

    if let Some(metadata) = dex_turn_metadata
        && let Ok(metadata) = HeaderValue::from_str(metadata)
    {
        headers.insert(X_DEX_TURN_METADATA_HEADER, metadata);
    }

    headers
}
