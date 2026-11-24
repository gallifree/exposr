//! MongoDB documents and the [`Store`] wrapper.

use crate::engine::{ClientCfg, ServiceCfg, Snapshot};
use anyhow::Result;
use futures::TryStreamExt;
use mongodb::bson::{DateTime, doc, oid::ObjectId};
use mongodb::options::{ClientOptions, IndexOptions};
use mongodb::{Collection, Database, IndexModel};
use proto::Protocol;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientDoc {
    #[serde(rename = "_id")]
    pub id: ObjectId,
    pub name: String,
    pub token_hash: String,
    pub enabled: bool,
    pub created_at: DateTime,
    #[serde(default)]
    pub last_seen_at: Option<DateTime>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceDoc {
    #[serde(rename = "_id")]
    pub id: ObjectId,
    pub name: String,
    pub client_id: ObjectId,
    pub protocol: Protocol,
    pub bind_port: i32,
    pub local_addr: String,
    pub enabled: bool,
    pub created_at: DateTime,
    /// Persisted traffic totals (flushed periodically from the engine).
    #[serde(default)]
    pub bytes_in: i64,
    #[serde(default)]
    pub bytes_out: i64,
    /// `http` services: SHA-256 of the secret in their public URL (`/t/<secret>/`).
    #[serde(default)]
    pub http_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserDoc {
    #[serde(rename = "_id")]
    pub id: ObjectId,
    pub username: String,
    pub password_hash: String,
    pub created_at: DateTime,
    #[serde(default)]
    pub last_login_at: Option<DateTime>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionDoc {
    /// SHA-256 of the cookie token.
    #[serde(rename = "_id")]
    pub id: String,
    pub user_id: ObjectId,
    pub expires_at: DateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventDoc {
    #[serde(rename = "_id")]
    pub id: ObjectId,
    pub at: DateTime,
    /// Machine-readable kind, e.g. `client.connected`, `service.created`.
    pub kind: String,
    /// Main subject (client/service/user name).
    pub subject: String,
    #[serde(default)]
    pub detail: Option<String>,
    /// Admin who performed the action, if any.
    #[serde(default)]
    pub actor: Option<String>,
}

#[derive(Clone)]
pub struct Store {
    db: Database,
}

impl Store {
    pub async fn connect(uri: &str, db_name: &str) -> Result<Self> {
        let mut opts = ClientOptions::parse(uri).await?;
        opts.app_name = Some("exposr-server".into());
        opts.server_selection_timeout = Some(Duration::from_secs(10));
        let client = mongodb::Client::with_options(opts)?;
        let db = client.database(db_name);
        db.run_command(doc! { "ping": 1 }).await?;
        let store = Store { db };
        store.ensure_indexes().await?;
        Ok(store)
    }

    pub fn clients(&self) -> Collection<ClientDoc> {
        self.db.collection("clients")
    }
    pub fn services(&self) -> Collection<ServiceDoc> {
        self.db.collection("services")
    }
    pub fn users(&self) -> Collection<UserDoc> {
        self.db.collection("users")
    }
    pub fn sessions(&self) -> Collection<SessionDoc> {
        self.db.collection("sessions")
    }
    pub fn events(&self) -> Collection<EventDoc> {
        self.db.collection("events")
    }
    /// Server-wide settings changed from the admin UI: one document, `_id: "server"`.
    fn settings(&self) -> Collection<mongodb::bson::Document> {
        self.db.collection("settings")
    }

    pub async fn allow_registration(&self) -> Result<bool> {
        let settings = self.settings().find_one(doc! { "_id": "server" }).await?;
        Ok(settings.and_then(|d| d.get_bool("allow_registration").ok()).unwrap_or(false))
    }

    pub async fn set_allow_registration(&self, enabled: bool) -> Result<()> {
        self.settings()
            .update_one(doc! { "_id": "server" }, doc! { "$set": { "allow_registration": enabled } })
            .upsert(true)
            .await?;
        Ok(())
    }

    async fn ensure_indexes(&self) -> Result<()> {
        let unique = |keys| IndexModel::builder().keys(keys).options(IndexOptions::builder().unique(true).build()).build();
        let ttl = |field: &str, secs| {
            IndexModel::builder()
                .keys(doc! { field: 1 })
                .options(IndexOptions::builder().expire_after(Duration::from_secs(secs)).build())
                .build()
        };
        self.clients().create_index(unique(doc! { "token_hash": 1 })).await?;
        self.clients().create_index(unique(doc! { "name": 1 })).await?;
        // Ports are unique among tcp/udp services only: http ones all have bind_port 0.
        let _ = self.services().drop_index("protocol_1_bind_port_1").await;
        let ports = IndexOptions::builder()
            .name("ports_unique".to_owned())
            .unique(true)
            .partial_filter_expression(doc! { "protocol": { "$in": ["tcp", "udp"] } })
            .build();
        self.services()
            .create_index(IndexModel::builder().keys(doc! { "protocol": 1, "bind_port": 1 }).options(ports).build())
            .await?;
        self.services()
            .create_index(
                IndexModel::builder()
                    .keys(doc! { "http_key": 1 })
                    .options(IndexOptions::builder().unique(true).sparse(true).build())
                    .build(),
            )
            .await?;
        self.services().create_index(unique(doc! { "name": 1 })).await?;
        self.services().create_index(IndexModel::builder().keys(doc! { "client_id": 1 }).build()).await?;
        self.users().create_index(unique(doc! { "username": 1 })).await?;
        self.sessions().create_index(ttl("expires_at", 0)).await?;
        // Keep 30 days of activity.
        self.events().create_index(ttl("at", 30 * 24 * 3600)).await?;
        Ok(())
    }

    pub async fn all_clients(&self) -> Result<Vec<ClientDoc>> {
        Ok(self.clients().find(doc! {}).sort(doc! { "name": 1 }).await?.try_collect().await?)
    }

    pub async fn all_services(&self) -> Result<Vec<ServiceDoc>> {
        Ok(self.services().find(doc! {}).sort(doc! { "name": 1 }).await?.try_collect().await?)
    }

    /// The desired configuration, in the form the engine consumes.
    pub async fn snapshot(&self) -> Result<Snapshot> {
        let clients = self
            .all_clients()
            .await?
            .into_iter()
            .map(|c| ClientCfg { id: c.id.to_hex(), name: c.name, token_hash: c.token_hash, enabled: c.enabled })
            .collect();
        let services = self
            .all_services()
            .await?
            .into_iter()
            .filter_map(|s| {
                Some(ServiceCfg {
                    id: s.id.to_hex(),
                    name: s.name,
                    client_id: s.client_id.to_hex(),
                    protocol: s.protocol,
                    bind_port: u16::try_from(s.bind_port).ok()?,
                    local_addr: s.local_addr,
                    enabled: s.enabled,
                    http_key: s.http_key,
                })
            })
            .collect();
        Ok(Snapshot { clients, services })
    }

    pub async fn log(&self, kind: &str, subject: &str, detail: Option<String>, actor: Option<&str>) {
        let ev = EventDoc {
            id: ObjectId::new(),
            at: DateTime::now(),
            kind: kind.to_owned(),
            subject: subject.to_owned(),
            detail,
            actor: actor.map(str::to_owned),
        };
        if let Err(e) = self.events().insert_one(ev).await {
            tracing::warn!("cannot log event: {e}");
        }
    }
}

/// True if `e` is a unique-index violation.
pub fn is_duplicate(e: &mongodb::error::Error) -> bool {
    use mongodb::error::{ErrorKind, WriteFailure};
    match &*e.kind {
        ErrorKind::Write(WriteFailure::WriteError(w)) => w.code == 11000,
        ErrorKind::Command(c) => c.code == 11000,
        _ => false,
    }
}
