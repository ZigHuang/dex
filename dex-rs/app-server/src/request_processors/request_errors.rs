use super::*;
use dex_protocol::error::DexErrorDetails;

pub(super) fn environment_selection_error(err: DexErr) -> JSONRPCErrorError {
    match err.details() {
        DexErrorDetails::InvalidRequest(message) => invalid_request(message.clone()),
        _ => internal_error(format!("failed to validate environment selections: {err}")),
    }
}
