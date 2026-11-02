//! Reads the root authorization version selected by the extension context policy.

use dex_guardian_reviewer::ReviewContextPolicy;

use crate::dex_thread::GuardianAuthorizationVersion;
use crate::session::session::Session;

pub(super) async fn root_review_version(
    policy: ReviewContextPolicy,
    session: &Session,
) -> Option<(GuardianAuthorizationVersion, u64)> {
    if matches!(
        policy,
        ReviewContextPolicy::Legacy | ReviewContextPolicy::LegacyWithCheckpointReuse
    ) {
        return None;
    }
    session
        .services
        .agent_control
        .get_guardian_package(session.thread_id)
        .await
        .map(|snapshot| {
            (
                snapshot.authorization_version,
                snapshot.review_context_revision,
            )
        })
}
