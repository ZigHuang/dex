use color_eyre::eyre::Result;
use color_eyre::eyre::eyre;
use dex_app_server_client::AppServerPath;
use dex_app_server_client::AppServerRequestHandle;
use dex_app_server_protocol::FeedbackUploadParams;
use dex_app_server_protocol::FeedbackUploadResponse;
use dex_feedback::DexFeedback;

pub(super) async fn fetch_feedback_upload(
    request_handle: AppServerRequestHandle,
    dex_home: Option<AppServerPath>,
    params: FeedbackUploadParams,
    feedback: DexFeedback,
) -> Result<FeedbackUploadResponse> {
    let _ = (request_handle, dex_home, params, feedback);
    Err(eyre!("Feedback uploads are not supported by Dex."))
}
