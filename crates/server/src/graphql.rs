//! Admin GraphQL API.
//!
//! Errors are returned as stable codes (`PORT_TAKEN`, `UNAUTHENTICATED`, …) in
//! the message so the UI can translate them.

use crate::app::App;
use crate::auth::{self, AuthUser};
use crate::db::{ClientDoc, EventDoc, ServiceDoc, UserDoc, is_duplicate};
use crate::engine::ListenerState;
use async_graphql::{
    Context, Enum, Error, ID, InputObject, Object, Result, Schema, SimpleObject, Subscription,
};
use futures::{Stream, TryStreamExt};
use mongodb::bson::{DateTime, Document, doc, oid::ObjectId};
use std::sync::Arc;
use std::time::Duration;

pub type AppSchema = Schema<Query, Mutation, Subscription>;

pub fn schema(app: Arc<App>) -> AppSchema {
    Schema::build(Query, Mutation, Subscription).data(app).limit_depth(12).limit_complexity(2000).finish()
}

pub fn sdl() -> String {
    Schema::build(Query, Mutation, Subscription).finish().sdl()
}

/// Per-request data inserted by the HTTP layer.
pub struct RequestMeta {
    pub https: bool,
}

fn err(code: &str) -> Error {
    Error::new(code)
}

fn internal<E: std::fmt::Display>(e: E) -> Error {
    tracing::error!("internal error: {e}");
    err("INTERNAL")
}

fn app<'a>(ctx: &Context<'a>) -> &'a Arc<App> {
    ctx.data_unchecked::<Arc<App>>()
}

fn require<'a>(ctx: &Context<'a>) -> Result<&'a AuthUser> {
    ctx.data_opt::<Option<AuthUser>>().and_then(Option::as_ref).ok_or_else(|| err("UNAUTHENTICATED"))
}

fn oid(id: &ID) -> Result<ObjectId> {
    ObjectId::parse_str(id.as_str()).map_err(|_| err("NOT_FOUND"))
}

fn ts(d: DateTime) -> String {
    d.try_to_rfc3339_string().unwrap_or_default()
}

fn valid_name(name: &str) -> Result<String> {
    let name = name.trim();
    let ok = !name.is_empty()
        && name.len() <= 64
        && name.chars().all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | ' '));
    if ok { Ok(name.to_owned()) } else { Err(err("INVALID_NAME")) }
}

fn valid_local_addr(addr: &str) -> Result<String> {
    let addr = addr.trim();
    let ok = addr
        .rsplit_once(':')
        .is_some_and(|(host, port)| !host.is_empty() && port.parse::<u16>().is_ok_and(|p| p > 0));
    if ok { Ok(addr.to_owned()) } else { Err(err("INVALID_LOCAL_ADDR")) }
}

// ------------------------------------------------------------------ types

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum Protocol {
    Tcp,
    Udp,
}

impl From<proto::Protocol> for Protocol {
    fn from(p: proto::Protocol) -> Self {
        match p {
            proto::Protocol::Tcp => Protocol::Tcp,
            proto::Protocol::Udp => Protocol::Udp,
        }
    }
}

impl From<Protocol> for proto::Protocol {
    fn from(p: Protocol) -> Self {
        match p {
            Protocol::Tcp => proto::Protocol::Tcp,
            Protocol::Udp => proto::Protocol::Udp,
        }
    }
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum ServiceStatus {
    /// Disabled, or its client is disabled.
    Inactive,
    Listening,
    /// The public port could not be bound (see `error`).
    Failed,
}

pub struct Client(ClientDoc);

#[Object]
impl Client {
    async fn id(&self) -> ID {
        ID(self.0.id.to_hex())
    }
    async fn name(&self) -> &str {
        &self.0.name
    }
    async fn enabled(&self) -> bool {
        self.0.enabled
    }
    async fn created_at(&self) -> String {
        ts(self.0.created_at)
    }
    async fn last_seen_at(&self) -> Option<String> {
        self.0.last_seen_at.map(ts)
    }
    async fn online(&self, ctx: &Context<'_>) -> bool {
        app(ctx).engine.session_info(&self.0.id.to_hex()).is_some()
    }
    async fn connected_at(&self, ctx: &Context<'_>) -> Option<String> {
        let info = app(ctx).engine.session_info(&self.0.id.to_hex())?;
        Some(ts(DateTime::from_system_time(info.connected_at)))
    }
    async fn remote_addr(&self, ctx: &Context<'_>) -> Option<String> {
        app(ctx).engine.session_info(&self.0.id.to_hex()).map(|i| i.addr.to_string())
    }
    async fn client_version(&self, ctx: &Context<'_>) -> Option<String> {
        app(ctx).engine.session_info(&self.0.id.to_hex()).map(|i| i.client_version)
    }
    async fn services(&self, ctx: &Context<'_>) -> Result<Vec<Service>> {
        find_services(ctx, doc! { "client_id": self.0.id }).await
    }
}

pub struct Service(ServiceDoc);

#[Object]
impl Service {
    async fn id(&self) -> ID {
        ID(self.0.id.to_hex())
    }
    async fn name(&self) -> &str {
        &self.0.name
    }
    async fn client_id(&self) -> ID {
        ID(self.0.client_id.to_hex())
    }
    async fn client(&self, ctx: &Context<'_>) -> Result<Option<Client>> {
        let c = app(ctx).store.clients().find_one(doc! { "_id": self.0.client_id }).await.map_err(internal)?;
        Ok(c.map(Client))
    }
    async fn protocol(&self) -> Protocol {
        self.0.protocol.into()
    }
    async fn bind_port(&self) -> i32 {
        self.0.bind_port
    }
    async fn local_addr(&self) -> &str {
        &self.0.local_addr
    }
    async fn enabled(&self) -> bool {
        self.0.enabled
    }
    async fn created_at(&self) -> String {
        ts(self.0.created_at)
    }
    async fn runtime(&self, ctx: &Context<'_>) -> ServiceLive {
        live_for(app(ctx), &self.0)
    }
}

/// Live state of one service (polled or pushed via the `live` subscription).
#[derive(SimpleObject, Clone)]
pub struct ServiceLive {
    pub id: ID,
    pub status: ServiceStatus,
    pub error: Option<String>,
    pub active_connections: u64,
    /// Connections since the server started.
    pub total_connections: u64,
    /// Total bytes visitor → service, all time.
    pub bytes_in: u64,
    /// Total bytes service → visitor, all time.
    pub bytes_out: u64,
}

fn live_for(app: &App, s: &ServiceDoc) -> ServiceLive {
    let id = s.id.to_hex();
    let rt = app.engine.service_runtime(&id);
    let (un_in, un_out) = app.unflushed(&id);
    ServiceLive {
        id: ID(id),
        status: match rt.state {
            ListenerState::Inactive => ServiceStatus::Inactive,
            ListenerState::Listening => ServiceStatus::Listening,
            ListenerState::Failed => ServiceStatus::Failed,
        },
        error: rt.error,
        active_connections: rt.active_connections,
        total_connections: rt.total_connections,
        bytes_in: s.bytes_in.max(0) as u64 + un_in,
        bytes_out: s.bytes_out.max(0) as u64 + un_out,
    }
}

#[derive(SimpleObject)]
pub struct User {
    pub id: ID,
    pub username: String,
    pub created_at: String,
    pub last_login_at: Option<String>,
}

impl From<UserDoc> for User {
    fn from(u: UserDoc) -> Self {
        User { id: ID(u.id.to_hex()), username: u.username, created_at: ts(u.created_at), last_login_at: u.last_login_at.map(ts) }
    }
}

#[derive(SimpleObject)]
pub struct Event {
    pub id: ID,
    pub at: String,
    pub kind: String,
    pub subject: String,
    pub detail: Option<String>,
    pub actor: Option<String>,
}

impl From<EventDoc> for Event {
    fn from(e: EventDoc) -> Self {
        Event { id: ID(e.id.to_hex()), at: ts(e.at), kind: e.kind, subject: e.subject, detail: e.detail, actor: e.actor }
    }
}

#[derive(SimpleObject)]
pub struct ServerInfo {
    pub version: String,
    pub control_port: i32,
    /// Hostname clients should connect to (`PUBLIC_HOST`), if configured.
    pub public_host: Option<String>,
    /// SHA-256 fingerprint of the tunnel TLS certificate, for client pinning.
    pub fingerprint: String,
    pub uptime_seconds: u64,
    /// Allowed public ports (`SERVICE_PORT_RANGE`), if restricted.
    pub port_range_start: Option<i32>,
    pub port_range_end: Option<i32>,
}

#[derive(SimpleObject, Clone)]
pub struct Live {
    pub online_client_ids: Vec<ID>,
    pub services: Vec<ServiceLive>,
    pub clients_total: i32,
    pub services_total: i32,
    pub services_listening: i32,
    pub active_connections: u64,
    pub bytes_in: u64,
    pub bytes_out: u64,
}

async fn live_snapshot(app: &App) -> Result<Live> {
    let services = app.store.all_services().await.map_err(internal)?;
    let clients_total = app.store.clients().count_documents(doc! {}).await.map_err(internal)? as i32;
    let services: Vec<ServiceLive> = services.iter().map(|s| live_for(app, s)).collect();
    Ok(Live {
        online_client_ids: app.engine.online_clients().into_iter().map(ID).collect(),
        clients_total,
        services_total: services.len() as i32,
        services_listening: services.iter().filter(|s| s.status == ServiceStatus::Listening).count() as i32,
        active_connections: services.iter().map(|s| s.active_connections).sum(),
        bytes_in: services.iter().map(|s| s.bytes_in).sum(),
        bytes_out: services.iter().map(|s| s.bytes_out).sum(),
        services,
    })
}

async fn find_services(ctx: &Context<'_>, filter: Document) -> Result<Vec<Service>> {
    let docs: Vec<ServiceDoc> = app(ctx)
        .store
        .services()
        .find(filter)
        .sort(doc! { "name": 1 })
        .await
        .map_err(internal)?
        .try_collect()
        .await
        .map_err(internal)?;
    Ok(docs.into_iter().map(Service).collect())
}

// ------------------------------------------------------------------ query

pub struct Query;

#[Object]
impl Query {
    /// The signed-in admin, or null.
    async fn me(&self, ctx: &Context<'_>) -> Option<User> {
        let user = ctx.data_opt::<Option<AuthUser>>()?.as_ref()?;
        let doc = app(ctx).store.users().find_one(doc! { "_id": user.id }).await.ok()??;
        Some(doc.into())
    }

    async fn server_info(&self, ctx: &Context<'_>) -> Result<ServerInfo> {
        require(ctx)?;
        let app = app(ctx);
        Ok(ServerInfo {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            control_port: app.control_port.into(),
            public_host: app.public_host.clone(),
            fingerprint: app.fingerprint.clone(),
            uptime_seconds: app.started_at.elapsed().as_secs(),
            port_range_start: app.port_range.map(|r| r.0.into()),
            port_range_end: app.port_range.map(|r| r.1.into()),
        })
    }

    async fn clients(&self, ctx: &Context<'_>) -> Result<Vec<Client>> {
        require(ctx)?;
        Ok(app(ctx).store.all_clients().await.map_err(internal)?.into_iter().map(Client).collect())
    }

    async fn client(&self, ctx: &Context<'_>, id: ID) -> Result<Option<Client>> {
        require(ctx)?;
        let c = app(ctx).store.clients().find_one(doc! { "_id": oid(&id)? }).await.map_err(internal)?;
        Ok(c.map(Client))
    }

    async fn services(&self, ctx: &Context<'_>) -> Result<Vec<Service>> {
        require(ctx)?;
        find_services(ctx, doc! {}).await
    }

    async fn users(&self, ctx: &Context<'_>) -> Result<Vec<User>> {
        require(ctx)?;
        let users: Vec<UserDoc> = app(ctx)
            .store
            .users()
            .find(doc! {})
            .sort(doc! { "username": 1 })
            .await
            .map_err(internal)?
            .try_collect()
            .await
            .map_err(internal)?;
        Ok(users.into_iter().map(Into::into).collect())
    }

    async fn events(&self, ctx: &Context<'_>, #[graphql(default = 50)] limit: i64) -> Result<Vec<Event>> {
        require(ctx)?;
        let events: Vec<EventDoc> = app(ctx)
            .store
            .events()
            .find(doc! {})
            .sort(doc! { "at": -1 })
            .limit(limit.clamp(1, 500))
            .await
            .map_err(internal)?
            .try_collect()
            .await
            .map_err(internal)?;
        Ok(events.into_iter().map(Into::into).collect())
    }

    async fn live(&self, ctx: &Context<'_>) -> Result<Live> {
        require(ctx)?;
        live_snapshot(app(ctx)).await
    }
}

// --------------------------------------------------------------- mutation

#[derive(InputObject)]
pub struct CreateServiceInput {
    pub name: String,
    pub client_id: ID,
    pub protocol: Protocol,
    pub bind_port: i32,
    /// `host:port` reachable from the client, e.g. `127.0.0.1:8080`.
    pub local_addr: String,
    #[graphql(default = true)]
    pub enabled: bool,
}

#[derive(InputObject)]
pub struct UpdateServiceInput {
    pub name: Option<String>,
    pub client_id: Option<ID>,
    pub protocol: Option<Protocol>,
    pub bind_port: Option<i32>,
    pub local_addr: Option<String>,
    pub enabled: Option<bool>,
}

#[derive(InputObject)]
pub struct UpdateClientInput {
    pub name: Option<String>,
    pub enabled: Option<bool>,
}

/// Returned when a client is created or its token regenerated. The token is
/// only ever shown here.
#[derive(SimpleObject)]
pub struct ClientWithToken {
    pub client: Client,
    pub token: String,
}

fn check_port(app: &App, protocol: Protocol, port: i32) -> Result<()> {
    if !(1..=65535).contains(&port) {
        return Err(err("INVALID_PORT"));
    }
    if let Some((lo, hi)) = app.port_range
        && !(i32::from(lo)..=i32::from(hi)).contains(&port)
    {
        return Err(err("PORT_OUT_OF_RANGE"));
    }
    if protocol == Protocol::Tcp && (port == i32::from(app.control_port) || port == i32::from(app.admin_port)) {
        return Err(err("RESERVED_PORT"));
    }
    Ok(())
}

fn write_err(e: mongodb::error::Error, dup: &str) -> Error {
    if is_duplicate(&e) {
        // Unique indexes: (protocol, bind_port) and name.
        if e.to_string().contains("bind_port") { err("PORT_TAKEN") } else { err(dup) }
    } else {
        internal(e)
    }
}

async fn client_exists(app: &App, id: ObjectId) -> Result<()> {
    match app.store.clients().find_one(doc! { "_id": id }).await.map_err(internal)? {
        Some(_) => Ok(()),
        None => Err(err("NOT_FOUND")),
    }
}

pub struct Mutation;

#[Object]
impl Mutation {
    async fn login(&self, ctx: &Context<'_>, username: String, password: String) -> Result<User> {
        let app = app(ctx);
        let user = app.store.users().find_one(doc! { "username": username.trim() }).await.map_err(internal)?;
        let Some(user) = user.filter(|u| auth::verify_password(&password, &u.password_hash)) else {
            // Constant-ish time: hash anyway so timing doesn't reveal valid usernames.
            let _ = auth::hash_password(&password);
            app.store.log("auth.failed", username.trim(), None, None).await;
            return Err(err("INVALID_CREDENTIALS"));
        };
        let token = auth::create_session(&app.store, user.id).await.map_err(internal)?;
        let https = ctx.data_opt::<RequestMeta>().is_some_and(|m| m.https);
        ctx.insert_http_header(axum::http::header::SET_COOKIE, auth::session_cookie(&token, https));
        let _ = app
            .store
            .users()
            .update_one(doc! { "_id": user.id }, doc! { "$set": { "last_login_at": DateTime::now() } })
            .await;
        app.store.log("auth.login", &user.username, None, Some(&user.username)).await;
        Ok(user.into())
    }

    async fn logout(&self, ctx: &Context<'_>) -> bool {
        if let Some(token) = ctx.data_opt::<SessionToken>().and_then(|t| t.0.clone()) {
            let _ = app(ctx).store.sessions().delete_one(doc! { "_id": proto::sha256_hex(token.as_bytes()) }).await;
        }
        ctx.insert_http_header(axum::http::header::SET_COOKIE, auth::clear_cookie());
        true
    }

    async fn change_password(&self, ctx: &Context<'_>, current: String, new_password: String) -> Result<bool> {
        let me = require(ctx)?;
        let app = app(ctx);
        let user = app.store.users().find_one(doc! { "_id": me.id }).await.map_err(internal)?.ok_or_else(|| err("NOT_FOUND"))?;
        if !auth::verify_password(&current, &user.password_hash) {
            return Err(err("WRONG_PASSWORD"));
        }
        if new_password.chars().count() < 8 {
            return Err(err("WEAK_PASSWORD"));
        }
        let hash = auth::hash_password(&new_password).map_err(internal)?;
        app.store.users().update_one(doc! { "_id": me.id }, doc! { "$set": { "password_hash": hash } }).await.map_err(internal)?;
        // Sign out every other session of this user.
        let current_hash = ctx.data_opt::<SessionToken>().and_then(|t| t.0.as_ref()).map(|t| proto::sha256_hex(t.as_bytes()));
        let _ = app.store.sessions().delete_many(doc! { "user_id": me.id, "_id": { "$ne": current_hash } }).await;
        app.store.log("user.password_changed", &me.username, None, Some(&me.username)).await;
        Ok(true)
    }

    async fn create_user(&self, ctx: &Context<'_>, username: String, password: String) -> Result<User> {
        let me = require(ctx)?;
        let username = valid_name(&username)?;
        if password.chars().count() < 8 {
            return Err(err("WEAK_PASSWORD"));
        }
        let user = UserDoc {
            id: ObjectId::new(),
            username,
            password_hash: auth::hash_password(&password).map_err(internal)?,
            created_at: DateTime::now(),
            last_login_at: None,
        };
        app(ctx).store.users().insert_one(&user).await.map_err(|e| write_err(e, "USERNAME_TAKEN"))?;
        app(ctx).store.log("user.created", &user.username, None, Some(&me.username)).await;
        Ok(user.into())
    }

    async fn delete_user(&self, ctx: &Context<'_>, id: ID) -> Result<bool> {
        let me = require(ctx)?;
        let id = oid(&id)?;
        if id == me.id {
            return Err(err("CANNOT_DELETE_SELF"));
        }
        let app = app(ctx);
        let user = app.store.users().find_one_and_delete(doc! { "_id": id }).await.map_err(internal)?.ok_or_else(|| err("NOT_FOUND"))?;
        let _ = app.store.sessions().delete_many(doc! { "user_id": id }).await;
        app.store.log("user.deleted", &user.username, None, Some(&me.username)).await;
        Ok(true)
    }

    async fn create_client(&self, ctx: &Context<'_>, name: String) -> Result<ClientWithToken> {
        let me = require(ctx)?;
        let app = app(ctx);
        let token = auth::random_token(24);
        let client = ClientDoc {
            id: ObjectId::new(),
            name: valid_name(&name)?,
            token_hash: proto::sha256_hex(token.as_bytes()),
            enabled: true,
            created_at: DateTime::now(),
            last_seen_at: None,
        };
        app.store.clients().insert_one(&client).await.map_err(|e| write_err(e, "NAME_TAKEN"))?;
        app.store.log("client.created", &client.name, None, Some(&me.username)).await;
        app.resync().await;
        Ok(ClientWithToken { client: Client(client), token })
    }

    async fn update_client(&self, ctx: &Context<'_>, id: ID, input: UpdateClientInput) -> Result<Client> {
        let me = require(ctx)?;
        let app = app(ctx);
        let mut set = Document::new();
        if let Some(name) = input.name {
            set.insert("name", valid_name(&name)?);
        }
        if let Some(enabled) = input.enabled {
            set.insert("enabled", enabled);
        }
        let opts = mongodb::options::FindOneAndUpdateOptions::builder()
            .return_document(mongodb::options::ReturnDocument::After)
            .build();
        let client = app
            .store
            .clients()
            .find_one_and_update(doc! { "_id": oid(&id)? }, doc! { "$set": set })
            .with_options(opts)
            .await
            .map_err(|e| write_err(e, "NAME_TAKEN"))?
            .ok_or_else(|| err("NOT_FOUND"))?;
        app.store.log("client.updated", &client.name, None, Some(&me.username)).await;
        app.resync().await;
        Ok(Client(client))
    }

    /// Issues a new token; the old one stops working and the client is disconnected.
    async fn regenerate_client_token(&self, ctx: &Context<'_>, id: ID) -> Result<ClientWithToken> {
        let me = require(ctx)?;
        let app = app(ctx);
        let token = auth::random_token(24);
        let opts = mongodb::options::FindOneAndUpdateOptions::builder()
            .return_document(mongodb::options::ReturnDocument::After)
            .build();
        let client = app
            .store
            .clients()
            .find_one_and_update(doc! { "_id": oid(&id)? }, doc! { "$set": { "token_hash": proto::sha256_hex(token.as_bytes()) } })
            .with_options(opts)
            .await
            .map_err(internal)?
            .ok_or_else(|| err("NOT_FOUND"))?;
        app.store.log("client.token_regenerated", &client.name, None, Some(&me.username)).await;
        app.resync().await;
        // Kick the current session: its token is no longer valid.
        app.engine.kick(&client.id.to_hex());
        Ok(ClientWithToken { client: Client(client), token })
    }

    /// Deletes a client and all its services.
    async fn delete_client(&self, ctx: &Context<'_>, id: ID) -> Result<bool> {
        let me = require(ctx)?;
        let app = app(ctx);
        let id = oid(&id)?;
        let client = app.store.clients().find_one_and_delete(doc! { "_id": id }).await.map_err(internal)?.ok_or_else(|| err("NOT_FOUND"))?;
        app.store.services().delete_many(doc! { "client_id": id }).await.map_err(internal)?;
        app.store.log("client.deleted", &client.name, None, Some(&me.username)).await;
        app.resync().await;
        Ok(true)
    }

    async fn create_service(&self, ctx: &Context<'_>, input: CreateServiceInput) -> Result<Service> {
        let me = require(ctx)?;
        let app = app(ctx);
        check_port(app, input.protocol, input.bind_port)?;
        let client_id = oid(&input.client_id)?;
        client_exists(app, client_id).await?;
        let service = ServiceDoc {
            id: ObjectId::new(),
            name: valid_name(&input.name)?,
            client_id,
            protocol: input.protocol.into(),
            bind_port: input.bind_port,
            local_addr: valid_local_addr(&input.local_addr)?,
            enabled: input.enabled,
            created_at: DateTime::now(),
            bytes_in: 0,
            bytes_out: 0,
        };
        app.store.services().insert_one(&service).await.map_err(|e| write_err(e, "NAME_TAKEN"))?;
        app.store.log("service.created", &service.name, Some(format!("{}/{}", service.bind_port, service.protocol)), Some(&me.username)).await;
        app.resync().await;
        Ok(Service(service))
    }

    async fn update_service(&self, ctx: &Context<'_>, id: ID, input: UpdateServiceInput) -> Result<Service> {
        let me = require(ctx)?;
        let app = app(ctx);
        let id = oid(&id)?;
        let current = app.store.services().find_one(doc! { "_id": id }).await.map_err(internal)?.ok_or_else(|| err("NOT_FOUND"))?;

        let protocol = input.protocol.unwrap_or(current.protocol.into());
        let port = input.bind_port.unwrap_or(current.bind_port);
        check_port(app, protocol, port)?;
        let mut set = doc! { "protocol": proto::Protocol::from(protocol).to_string(), "bind_port": port };
        if let Some(name) = input.name {
            set.insert("name", valid_name(&name)?);
        }
        if let Some(cid) = input.client_id {
            let cid = oid(&cid)?;
            client_exists(app, cid).await?;
            set.insert("client_id", cid);
        }
        if let Some(addr) = input.local_addr {
            set.insert("local_addr", valid_local_addr(&addr)?);
        }
        if let Some(enabled) = input.enabled {
            set.insert("enabled", enabled);
        }
        let opts = mongodb::options::FindOneAndUpdateOptions::builder()
            .return_document(mongodb::options::ReturnDocument::After)
            .build();
        let service = app
            .store
            .services()
            .find_one_and_update(doc! { "_id": id }, doc! { "$set": set })
            .with_options(opts)
            .await
            .map_err(|e| write_err(e, "NAME_TAKEN"))?
            .ok_or_else(|| err("NOT_FOUND"))?;
        app.store.log("service.updated", &service.name, None, Some(&me.username)).await;
        app.resync().await;
        Ok(Service(service))
    }

    async fn delete_service(&self, ctx: &Context<'_>, id: ID) -> Result<bool> {
        let me = require(ctx)?;
        let app = app(ctx);
        let service = app.store.services().find_one_and_delete(doc! { "_id": oid(&id)? }).await.map_err(internal)?.ok_or_else(|| err("NOT_FOUND"))?;
        app.store.log("service.deleted", &service.name, None, Some(&me.username)).await;
        app.resync().await;
        Ok(true)
    }
}

/// The raw session cookie of the request (needed for logout).
pub struct SessionToken(pub Option<String>);

// ----------------------------------------------------------- subscription

pub struct Subscription;

#[Subscription]
impl Subscription {
    /// Pushes a fresh [`Live`] snapshot every `intervalMs` (min 500).
    async fn live(&self, ctx: &Context<'_>, #[graphql(default = 2000)] interval_ms: u64) -> Result<impl Stream<Item = Live>> {
        require(ctx)?;
        let app = app(ctx).clone();
        let period = Duration::from_millis(interval_ms.max(500));
        let ticks = tokio_stream::wrappers::IntervalStream::new(tokio::time::interval(period));
        Ok(futures::StreamExt::filter_map(ticks, move |_| {
            let app = app.clone();
            async move { live_snapshot(&app).await.ok() }
        }))
    }
}
