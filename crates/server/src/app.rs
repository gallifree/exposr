//! Glue between MongoDB and the engine: resync, stats persistence, event log.

use crate::db::Store;
use crate::engine::{Engine, EngineEvent};
use mongodb::bson::{DateTime, doc, oid::ObjectId};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::warn;

pub struct App {
    pub store: Store,
    pub engine: Engine,
    pub fingerprint: String,
    pub control_port: u16,
    pub admin_port: u16,
    pub public_host: Option<String>,
    pub port_range: Option<(u16, u16)>,
    pub started_at: Instant,
    /// Engine traffic counters already added to the service documents.
    flushed: Mutex<HashMap<String, (u64, u64)>>,
    resync_lock: tokio::sync::Mutex<()>,
}

impl App {
    pub fn new(
        store: Store,
        engine: Engine,
        fingerprint: String,
        control_port: u16,
        admin_port: u16,
        public_host: Option<String>,
        port_range: Option<(u16, u16)>,
    ) -> Arc<Self> {
        Arc::new(App {
            store,
            engine,
            fingerprint,
            control_port,
            admin_port,
            public_host,
            port_range,
            started_at: Instant::now(),
            flushed: Mutex::default(),
            resync_lock: tokio::sync::Mutex::new(()),
        })
    }

    /// Reloads the configuration from MongoDB into the engine.
    pub async fn resync(&self) {
        let _guard = self.resync_lock.lock().await;
        match self.store.snapshot().await {
            Ok(snap) => self.engine.apply(snap).await,
            Err(e) => warn!("cannot load configuration: {e:#}"),
        }
    }

    /// Bytes not yet persisted for a service.
    pub fn unflushed(&self, service_id: &str) -> (u64, u64) {
        let totals = self.engine.traffic_totals();
        let (cur_in, cur_out) = totals.get(service_id).copied().unwrap_or_default();
        let (done_in, done_out) = self.flushed.lock().unwrap().get(service_id).copied().unwrap_or_default();
        (cur_in.saturating_sub(done_in), cur_out.saturating_sub(done_out))
    }

    async fn flush_traffic(&self) {
        for (id, (cur_in, cur_out)) in self.engine.traffic_totals() {
            let (done_in, done_out) = self.flushed.lock().unwrap().get(&id).copied().unwrap_or_default();
            let (d_in, d_out) = (cur_in.saturating_sub(done_in), cur_out.saturating_sub(done_out));
            if d_in == 0 && d_out == 0 {
                continue;
            }
            let Ok(oid) = ObjectId::parse_str(&id) else { continue };
            let update = doc! { "$inc": { "bytes_in": d_in as i64, "bytes_out": d_out as i64 } };
            match self.store.services().update_one(doc! { "_id": oid }, update).await {
                Ok(_) => {
                    self.flushed.lock().unwrap().insert(id, (cur_in, cur_out));
                }
                Err(e) => warn!("cannot persist traffic: {e}"),
            }
        }
    }

    async fn touch_clients(&self, ids: &[String]) {
        let ids: Vec<ObjectId> = ids.iter().filter_map(|id| ObjectId::parse_str(id).ok()).collect();
        if ids.is_empty() {
            return;
        }
        let _ = self
            .store
            .clients()
            .update_many(doc! { "_id": { "$in": ids } }, doc! { "$set": { "last_seen_at": DateTime::now() } })
            .await;
    }

    /// Spawns the background tasks: periodic resync, traffic flush, event log.
    pub fn spawn_background(self: &Arc<Self>) {
        let app = self.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(30));
            tick.tick().await;
            loop {
                tick.tick().await;
                // Picks up edits made directly in the database and retries failed binds.
                app.resync().await;
                app.touch_clients(&app.engine.online_clients()).await;
            }
        });

        let app = self.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(10));
            loop {
                tick.tick().await;
                app.flush_traffic().await;
            }
        });

        let app = self.clone();
        let mut events = self.engine.subscribe();
        tokio::spawn(async move {
            loop {
                let ev = match events.recv().await {
                    Ok(ev) => ev,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                };
                let s = &app.store;
                match ev {
                    EngineEvent::ClientConnected { client_id, name, addr } => {
                        app.touch_clients(&[client_id]).await;
                        s.log("client.connected", &name, Some(addr.to_string()), None).await;
                    }
                    EngineEvent::ClientDisconnected { client_id, name } => {
                        app.touch_clients(&[client_id]).await;
                        s.log("client.disconnected", &name, None, None).await;
                    }
                    EngineEvent::ClientRejected { addr, reason } => {
                        s.log("client.rejected", &addr.ip().to_string(), Some(reason), None).await;
                    }
                    EngineEvent::ListenerFailed { name, error, .. } => {
                        s.log("service.failed", &name, Some(error), None).await;
                    }
                }
            }
        });
    }

    /// Final traffic flush on shutdown.
    pub async fn shutdown(&self) {
        self.flush_traffic().await;
    }
}
