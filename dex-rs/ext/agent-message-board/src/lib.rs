//! Installs agent message-board tools over the shared board interface.
//! Storage, backend behavior and tool implementations live in `dex-message-board`.

mod extension;

pub use extension::install;

/// Storage selected by the agent consumer for opening, setup and permanent deletion.
pub fn local_board_options() -> dex_message_board::LocalBoardOptions {
    dex_message_board::LocalBoardOptions {
        storage_namespace: "agent_message_board".into(),
    }
}
