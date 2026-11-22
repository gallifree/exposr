//! Admin authentication: argon2 password hashes and opaque session cookies
//! (only the SHA-256 of the token is stored; sessions expire via a TTL index).

use crate::db::{SessionDoc, Store};
use anyhow::Result;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng};
use argon2::Argon2;
use axum::http::HeaderMap;
use mongodb::bson::{DateTime, doc, oid::ObjectId};
use std::time::{Duration, SystemTime};

pub const COOKIE: &str = "exposr_session";
pub const SESSION_TTL: Duration = Duration::from_secs(7 * 24 * 3600);

#[derive(Debug, Clone)]
pub struct AuthUser {
    pub id: ObjectId,
    pub username: String,
}

pub fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Ok(Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("hashing failed: {e}"))?
        .to_string())
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    PasswordHash::new(hash).is_ok_and(|h| Argon2::default().verify_password(password.as_bytes(), &h).is_ok())
}

/// A random URL-safe secret (hex), `bytes` long before encoding.
pub fn random_token(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::fill(&mut buf[..]);
    hex::encode(buf)
}

pub async fn create_session(store: &Store, user_id: ObjectId) -> Result<String> {
    let token = random_token(32);
    let expires_at = DateTime::from_system_time(SystemTime::now() + SESSION_TTL);
    store
        .sessions()
        .insert_one(SessionDoc { id: proto::sha256_hex(token.as_bytes()), user_id, expires_at })
        .await?;
    Ok(token)
}

pub fn session_cookie(token: &str, secure: bool) -> String {
    let secure = if secure { "; Secure" } else { "" };
    format!("{COOKIE}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={}{secure}", SESSION_TTL.as_secs())
}

pub fn clear_cookie() -> String {
    format!("{COOKIE}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0")
}

pub fn token_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(axum::http::header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|kv| kv.trim().split_once('='))
        .find(|(k, _)| *k == COOKIE)
        .map(|(_, v)| v.to_owned())
}

pub async fn user_from_headers(store: &Store, headers: &HeaderMap) -> Option<AuthUser> {
    let token = token_from_headers(headers)?;
    let session = store
        .sessions()
        .find_one(doc! { "_id": proto::sha256_hex(token.as_bytes()), "expires_at": { "$gt": DateTime::now() } })
        .await
        .ok()??;
    let user = store.users().find_one(doc! { "_id": session.user_id }).await.ok()??;
    Some(AuthUser { id: user.id, username: user.username })
}
