//! The vaults items can be in (personal, or a team's) and moving items
//! between them (see store/vaults.rs).

use std::sync::Arc;

use tauri::State;

use crate::cloud::engine::TeamSummary;
use crate::cloud::items::Keyring;
use crate::ssh::pool::{self, AppState};
use crate::store::vaults::{self, Moved};
use crate::Db;

/// The teams this computer is in, as of its last sync. Read locally, so the
/// hosts view can show team sections without asking the server.
#[tauri::command]
pub fn list_teams(db: State<Db>) -> Result<Vec<TeamSummary>, String> {
    let conn = db.0.lock().map_err(|_| "database busy")?;
    conn.prepare("SELECT id, name, role, epoch FROM teams ORDER BY name COLLATE NOCASE")
        .and_then(|mut s| {
            s.query_map([], |r| Ok(TeamSummary { id: r.get(0)?, name: r.get(1)?, role: r.get(2)?, epoch: r.get(3)? }))?
                .collect()
        })
        .map_err(|e| e.to_string())
}

/// Move hosts, or a group with its hosts, to `team_id`'s vault (None:
/// personal), with their credentials and port forwards
#[tauri::command]
pub async fn move_hosts(
    state: State<'_, Arc<AppState>>,
    db: State<'_, Db>,
    host_ids: Vec<i64>,
    group_id: Option<i64>,
    team_id: Option<String>,
) -> Result<Moved, String> {
    // Their forwards are replaced by copies: a running one stops first
    let forwards: Vec<i64> = {
        let conn = db.0.lock().map_err(|_| "database busy")?;
        let mut stmt = conn
            .prepare(
                "SELECT f.id FROM port_forwards f JOIN hosts h ON h.id = f.host_id \
                 WHERE h.id IN (SELECT value FROM json_each(?1)) OR (?2 IS NOT NULL AND h.group_id = ?2)",
            )
            .map_err(|e| e.to_string())?;
        let ids = serde_json::to_string(&host_ids).map_err(|e| e.to_string())?;
        let rows = stmt.query_map(rusqlite::params![ids, group_id], |r| r.get(0)).map_err(|e| e.to_string())?;
        rows.collect::<Result<_, _>>().map_err(|e| e.to_string())?
    };
    let active = pool::list_active_forwards(state.inner());
    for id in forwards.into_iter().filter(|id| active.contains(id)) {
        pool::forward_stop(state.inner().clone(), id).await;
    }

    let moved = {
        let conn = db.0.lock().map_err(|_| "database busy")?;
        let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
        let moved = vaults::move_hosts(&tx, &host_ids, group_id, team_id.as_deref())?;
        tx.commit().map_err(|e| e.to_string())?;
        moved
    };
    moved.write_secrets(&Keyring);
    Ok(moved)
}
