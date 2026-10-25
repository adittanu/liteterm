mod commands;
mod import;
mod local;
mod ssh;
mod store;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use ssh::pool::AppState;
use tauri::Manager;

pub struct Db(pub Mutex<rusqlite::Connection>);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let conn = store::db::open_db(&data_dir)
                .map_err(|e| format!("Could not open database: {e}"))?;

            let state = AppState {
                app: app.handle().clone(),
                db_path: data_dir.join("hosts.db"),
                next_session_id: std::sync::atomic::AtomicU32::new(1),
                sessions: Mutex::new(HashMap::new()),
                local_sessions: Mutex::new(HashMap::new()),
                pending_hostkey: Mutex::new(HashMap::new()),
                forwards: Mutex::new(HashMap::new()),
            };

            app.manage(Db(Mutex::new(conn)));
            app.manage(Arc::new(state));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::hosts::list_hosts,
            commands::hosts::list_groups,
            commands::hosts::upsert_host,
            commands::hosts::delete_host,
            commands::hosts::upsert_group,
            commands::hosts::delete_group,
            commands::hosts::forget_host_key,
            commands::hosts::list_known_hosts,
            commands::hosts::delete_known_host_entry,
            commands::hosts::list_session_logs,
            commands::hosts::clear_session_logs,
            commands::local_fs::local_drives,
            commands::local_fs::local_list,
            commands::credentials::list_credentials,
            commands::credentials::upsert_credential,
            commands::credentials::delete_credential,
            commands::import::import_ssh_preview,
            commands::import::import_ssh_apply,
            commands::session::connect_host,
            commands::session::connect_adhoc,
            commands::session::spawn_local,
            commands::session::confirm_host_key,
            commands::session::disconnect_session,
            commands::session::session_write,
            commands::session::session_resize,
            commands::sftp::sftp_open,
            commands::sftp::sftp_list,
            commands::sftp::sftp_mkdir,
            commands::sftp::sftp_remove,
            commands::sftp::sftp_rename,
            commands::sftp::sftp_upload,
            commands::sftp::sftp_download,
            commands::sftp::sftp_cancel_transfer,
            commands::sftp::sftp_cross_transfer,
            commands::snippets::list_snippets,
            commands::snippets::upsert_snippet,
            commands::snippets::delete_snippet,
            commands::notes::list_notes,
            commands::notes::create_note,
            commands::notes::update_note,
            commands::notes::set_note_pinned,
            commands::notes::delete_note,
            commands::forwards::list_forwards,
            commands::forwards::list_active_forwards,
            commands::forwards::upsert_forward,
            commands::forwards::delete_forward,
            commands::forwards::forward_start,
            commands::forwards::forward_stop,
            commands::settings::get_settings,
            commands::settings::set_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running LiteTerm");
}
