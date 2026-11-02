//! Header overrides for host-created threads on the Dex Responses endpoint.
//! Headers are scoped to a model and rechecked against current auth on each attempt.

use http::HeaderMap;

/// Seed in `StartThreadOptions::thread_extension_init` to supply Responses headers.
///
/// Applied only to the selected model on the OpenAI provider with Dex backend
/// auth, regardless of the base URL. The headers are sent on HTTP requests and
/// WebSocket handshakes; changes require a new socket. Other providers and
/// unrelated threads do not inherit these headers.
#[derive(Clone)]
pub struct DexResponsesHeaders {
    pub model: String,
    pub headers: HeaderMap,
}

impl std::fmt::Debug for DexResponsesHeaders {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DexResponsesHeaders")
            .field("model", &self.model)
            .field("header_names", &self.headers.keys())
            .finish()
    }
}
