mod applog;
mod cloud;
mod commands;
mod import;
mod local;
mod menu;
mod paths;
mod ssh;
mod store;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use ssh::pool::AppState;
use tauri::Manager;

pub struct Db(pub Mutex<rusqlite::Connection>);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // First thing: the installed build has no console, so a panic would
    // otherwise leave nothing behind (see applog.rs)
    applog::install_panic_hook();

    let builder = tauri::Builder::default().plugin(tauri_plugin_dialog::init());
    // Tauri's default macOS menu would claim Cmd+W (see menu.rs)
    #[cfg(target_os = "macos")]
    let builder = builder.menu(menu::macos);

    builder
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let conn = store::db::open_db(&data_dir)
                .map_err(|e| format!("Could not open database: {e}"))?;
            // Optional account: picks up a session this device kept
            let cloud = cloud::engine::Cloud::new(&conn);

            let state = AppState {
                app: app.handle().clone(),
                db_path: data_dir.join("hosts.db"),
                next_session_id: std::sync::atomic::AtomicU32::new(1),
                next_transfer_id: std::sync::atomic::AtomicU32::new(1),
                sessions: Mutex::new(HashMap::new()),
                local_sessions: Mutex::new(HashMap::new()),
                pending_hostkey: Mutex::new(HashMap::new()),
                forwards: Mutex::new(HashMap::new()),
            };

            app.manage(Db(Mutex::new(conn)));
            app.manage(Arc::new(state));
            app.manage(cloud.clone());
            cloud.start(app.handle().clone());
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
            commands::local_fs::local_places,
            commands::local_fs::local_mkdir,
            commands::local_fs::local_rename,
            commands::net::resolve_host,
            commands::credentials::list_credentials,
            commands::credentials::upsert_credential,
            commands::credentials::delete_credential,
            commands::import::import_ssh_preview,
            commands::import::import_ssh_apply,
            commands::import::import_file_preview,
            commands::import::import_file_apply,
            commands::import::export_hosts,
            commands::session::connect_host,
            commands::session::connect_adhoc,
            commands::session::spawn_local,
            commands::session::confirm_host_key,
            commands::session::reconnect_now,
            commands::session::disconnect_session,
            commands::session::session_write,
            commands::session::session_resize,
            commands::sftp::sftp_open,
            commands::sftp::sftp_list,
            commands::sftp::sftp_home,
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
            commands::notes::set_note_tags,
            commands::notes::set_note_host,
            commands::notes::reorder_notes,
            commands::notes::set_note_archived,
            commands::notes::delete_note,
            commands::notes::save_note_image,
            commands::forwards::list_forwards,
            commands::forwards::list_active_forwards,
            commands::forwards::upsert_forward,
            commands::forwards::delete_forward,
            commands::forwards::forward_start,
            commands::forwards::forward_stop,
            commands::cloud::cloud_status,
            commands::cloud::cloud_send_code,
            commands::cloud::cloud_verify_code,
            commands::cloud::cloud_create_vault,
            commands::cloud::cloud_unlock,
            commands::cloud::cloud_recover,
            commands::cloud::cloud_change_password,
            commands::cloud::cloud_reset_vault,
            commands::cloud::cloud_sign_out,
            commands::cloud::cloud_sync_now,
            commands::settings::get_settings,
            commands::settings::set_settings,
            commands::update::check_update,
            commands::update::download_update,
            commands::update::install_update,
            commands::update::open_release_page,
        ])
        .run(tauri::generate_context!())
        .expect("error while running LiteTerm");
}
