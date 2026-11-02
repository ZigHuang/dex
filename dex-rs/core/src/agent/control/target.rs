//! Resolves controller targets using the caller's registered identity.
//! Legacy callers can still supply their captured session source for path resolution.

use super::LocalAgentControl;
use super::LocalAgentRuntime;
use crate::agent::api::AgentTarget;
use dex_protocol::AgentPath;
use dex_protocol::ThreadId;
use dex_protocol::error::DexErr;
use dex_protocol::error::Result as DexResult;
use dex_protocol::protocol::SessionSource;

impl LocalAgentControl {
    pub(crate) fn resolve_target(
        &self,
        caller: ThreadId,
        target: &AgentTarget,
    ) -> DexResult<ThreadId> {
        match target {
            AgentTarget::Id(thread_id) => Ok(*thread_id),
            AgentTarget::Reference(reference) => {
                let caller = self.runtime.ensure_agent_known(caller)?;
                self.runtime.resolve_path_reference(
                    &caller.agent_path.unwrap_or_else(AgentPath::root),
                    reference,
                )
            }
        }
    }
}

impl LocalAgentRuntime {
    pub(crate) async fn resolve_agent_reference(
        &self,
        _current_thread_id: ThreadId,
        current_session_source: &SessionSource,
        agent_reference: &str,
    ) -> DexResult<ThreadId> {
        let current_agent_path = current_session_source
            .get_agent_path()
            .unwrap_or_else(AgentPath::root);
        self.resolve_path_reference(&current_agent_path, agent_reference)
    }

    pub(super) fn resolve_path_reference(
        &self,
        current_agent_path: &AgentPath,
        agent_reference: &str,
    ) -> DexResult<ThreadId> {
        let agent_path = current_agent_path
            .resolve(agent_reference)
            .map_err(DexErr::UnsupportedOperation)?;
        if let Some(thread_id) = self.registry.agent_id_for_path(&agent_path) {
            return Ok(thread_id);
        }
        Err(DexErr::UnsupportedOperation(format!(
            "live agent path `{}` not found",
            agent_path.as_str()
        )))
    }
}
