use dex_exec_server::ShellInfo;
use dex_exec_server::ShellSnapshotRequest;
use dex_features::Feature;
use dex_tools::UnifiedExecShellMode;
use dex_utils_path_uri::PathUri;

use super::ExecCommandRequest;
use super::UnifiedExecContext;
use crate::shell::ShellType;

pub(super) fn shell_snapshot_request(
    request: &ExecCommandRequest,
    cwd: &PathUri,
    context: &UnifiedExecContext,
) -> Option<ShellSnapshotRequest> {
    if !context.session.features().enabled(Feature::ShellSnapshotV2)
        || !request.turn_environment.shell_snapshot_v2_supported
        || request.turn_environment.selection.cwd != *cwd
        || !matches!(request.shell_mode, UnifiedExecShellMode::Direct)
        || !matches!(
            request.shell_type,
            ShellType::Bash | ShellType::Zsh | ShellType::Sh
        )
        || request.command.get(1).is_none_or(|flag| flag != "-lc")
    {
        return None;
    }

    Some(ShellSnapshotRequest {
        scope_id: format!(
            "{}:{}",
            context.session.thread_id(),
            request.turn_environment.selection.environment_id
        ),
        shell: ShellInfo {
            name: request.shell_type.name().to_string(),
            path: request.command.first()?.clone(),
        },
    })
}
