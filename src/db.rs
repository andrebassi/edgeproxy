use crate::model::{Backend, RoutingState};
use anyhow::Result;
use rusqlite::{Connection, Row};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{sleep, Duration};

fn row_to_backend(row: &Row) -> rusqlite::Result<Backend> {
    Ok(Backend {
        id: row.get(0)?,
        app: row.get(1)?,
        region: row.get(2)?,
        country: row.get(3)?,
        wg_ip: row.get(4)?,
        port: row.get::<_, i64>(5)? as u16,
        healthy: row.get::<_, i64>(6)? != 0,
        weight: row.get::<_, i64>(7)? as u8,
        soft_limit: row.get::<_, i64>(8)? as u32,
        hard_limit: row.get::<_, i64>(9)? as u32,
    })
}

fn load_routing_state_from_sqlite(db_path: &str, version: u64) -> Result<RoutingState> {
    let conn = Connection::open(db_path)?;

    let mut stmt = conn.prepare(
        "SELECT id, app, region, country, wg_ip, port, healthy, weight, soft_limit, hard_limit FROM backends WHERE deleted IS NULL OR deleted = 0",
    )?;

    let rows = stmt.query_map([], |row| row_to_backend(row))?;

    let mut backends = Vec::new();
    for row in rows {
        backends.push(row?);
    }

    Ok(RoutingState { version, backends })
}

pub async fn start_routing_sync_sqlite(
    routing: Arc<RwLock<RoutingState>>,
    db_path: String,
    interval_secs: u64,
) -> Result<()> {
    let mut local_version: u64 = 0;

    loop {
        let db_path_clone = db_path.clone();
        match tokio::task::spawn_blocking(move || {
            load_routing_state_from_sqlite(&db_path_clone, local_version + 1)
        })
        .await
        {
            Ok(Ok(new_state)) => {
                {
                    let mut guard = routing.write().await;
                    *guard = new_state;
                    local_version = guard.version;
                }
                tracing::info!(
                    "routing reload ok, version={} backends={}",
                    local_version,
                    routing.read().await.backends.len()
                );
            }
            Ok(Err(e)) => tracing::error!("error reading routing: {:?}", e),
            Err(e) => tracing::error!("spawn_blocking error: {:?}", e),
        }

        sleep(Duration::from_secs(interval_secs)).await;
    }
}
