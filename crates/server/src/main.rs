use anyhow::{Context, Result};
use clap::Parser;
use mongodb::bson::{DateTime, doc, oid::ObjectId};
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use tracing::{info, warn};
use exposr_server::{app::App, auth, db, engine::Engine, graphql, tls, web};

/// Tunnel server: public listeners + admin UI/API, configured from MongoDB.
#[derive(Parser)]
#[command(version)]
struct Args {
    #[arg(long, env = "MONGODB_URI", default_value = "mongodb://localhost:27017")]
    mongodb_uri: String,
    #[arg(long, env = "MONGODB_DB", default_value = "exposr")]
    mongodb_db: String,
    /// Where tunnel clients connect (TLS).
    #[arg(long, env = "CONTROL_ADDR", default_value = "0.0.0.0:2333")]
    control_addr: SocketAddr,
    /// Admin UI + GraphQL API (plain HTTP; put a TLS proxy in front).
    #[arg(long, env = "ADMIN_ADDR", default_value = "0.0.0.0:8080")]
    admin_addr: SocketAddr,
    /// IP the public service ports are bound on.
    #[arg(long, env = "SERVICE_BIND_IP", default_value = "0.0.0.0")]
    service_bind_ip: IpAddr,
    /// Allowed public ports, e.g. `20000-20099` (must match the ports published by Docker).
    #[arg(long, env = "SERVICE_PORT_RANGE", value_parser = parse_range)]
    service_port_range: Option<(u16, u16)>,
    /// Hostname shown to admins for client configuration.
    #[arg(long, env = "PUBLIC_HOST")]
    public_host: Option<String>,
    /// Persistent data (self-signed certificate).
    #[arg(long, env = "DATA_DIR", default_value = "./data")]
    data_dir: PathBuf,
    #[arg(long, env = "TLS_CERT")]
    tls_cert: Option<PathBuf>,
    #[arg(long, env = "TLS_KEY")]
    tls_key: Option<PathBuf>,
    /// Built admin UI (Vike prerendered output).
    #[arg(long, env = "WEB_DIR", default_value = "./web/dist/client")]
    web_dir: PathBuf,
    /// First admin account, created only when there are no users yet.
    #[arg(long, env = "ADMIN_USERNAME", default_value = "admin")]
    admin_username: String,
    #[arg(long, env = "ADMIN_PASSWORD", hide_env_values = true)]
    admin_password: Option<String>,
    /// Let anyone create a client with an HTTP tunnel to its 127.0.0.1:8765
    /// through `POST /api/register` (used by the Rustart launcher's MCP server).
    #[arg(long, env = "ALLOW_REGISTRATION")]
    allow_registration: bool,
    /// Print the GraphQL schema (SDL) and exit.
    #[arg(long)]
    print_schema: bool,
}

fn parse_range(s: &str) -> Result<(u16, u16), String> {
    let (a, b) = s.split_once('-').unwrap_or((s, s));
    let (a, b): (u16, u16) = (a.trim().parse().map_err(|_| "bad start")?, b.trim().parse().map_err(|_| "bad end")?);
    if a == 0 || a > b {
        return Err("expected START-END with 0 < START <= END".into());
    }
    Ok((a, b))
}

async fn bootstrap_admin(store: &db::Store, args: &Args) -> Result<()> {
    if store.users().count_documents(doc! {}).await? > 0 {
        return Ok(());
    }
    let (password, generated) = match &args.admin_password {
        Some(p) if !p.is_empty() => (p.clone(), false),
        _ => (auth::random_token(12), true),
    };
    store
        .users()
        .insert_one(db::UserDoc {
            id: ObjectId::new(),
            username: args.admin_username.clone(),
            password_hash: auth::hash_password(&password)?,
            created_at: DateTime::now(),
            last_login_at: None,
        })
        .await?;
    if generated {
        warn!("created admin user \"{}\" with generated password: {password}", args.admin_username);
        warn!("change it after signing in, or set ADMIN_PASSWORD before the first start");
    } else {
        info!("created admin user \"{}\"", args.admin_username);
    }
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = tokio::signal::ctrl_c();
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("signal handler");
        tokio::select! { _ = ctrl_c => {}, _ = term.recv() => {} }
    }
    #[cfg(not(unix))]
    let _ = ctrl_c.await;
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();
    let args = Args::parse();
    if args.print_schema {
        print!("{}", graphql::sdl());
        return Ok(());
    }

    let store = db::Store::connect(&args.mongodb_uri, &args.mongodb_db).await.context("cannot connect to MongoDB")?;
    info!("connected to MongoDB database \"{}\"", args.mongodb_db);
    bootstrap_admin(&store, &args).await?;

    let tls = tls::load(args.tls_cert.clone(), args.tls_key.clone(), &args.data_dir)?;
    info!("tunnel certificate fingerprint: {}", tls.fingerprint);

    let engine = Engine::new(args.service_bind_ip);
    let app = App::new(
        store,
        engine.clone(),
        tls.fingerprint.clone(),
        args.control_addr.port(),
        args.admin_addr.port(),
        args.public_host.clone(),
        args.service_port_range,
        args.allow_registration,
    );
    app.resync().await;
    app.spawn_background();

    let control = tokio::net::TcpListener::bind(args.control_addr)
        .await
        .with_context(|| format!("cannot bind control port {}", args.control_addr))?;
    tokio::spawn(async move {
        if let Err(e) = engine.run_control(control, tls.acceptor).await {
            tracing::error!("control listener stopped: {e:#}");
            std::process::exit(1);
        }
    });

    if !args.web_dir.join("index.html").exists() {
        warn!("admin UI not found in {} (API still available)", args.web_dir.display());
    }
    let router = web::router(app.clone(), graphql::schema(app.clone()), args.web_dir.clone())
        .layer(tower_http::trace::TraceLayer::new_for_http());
    let http = tokio::net::TcpListener::bind(args.admin_addr)
        .await
        .with_context(|| format!("cannot bind admin port {}", args.admin_addr))?;
    info!("admin UI on http://{}", args.admin_addr);
    axum::serve(http, router).with_graceful_shutdown(shutdown_signal()).await?;

    app.shutdown().await;
    info!("bye");
    Ok(())
}
