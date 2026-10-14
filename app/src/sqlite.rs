//! SQLite implementation of rest_api's repository traits, so its services can run without Postgres.
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use rest_api::database::session::{Session, SessionsRepository};
use rest_api::database::user::{User, UsersRepository};
use rusqlite::{params, Connection, OptionalExtension};
use std::sync::{Mutex, MutexGuard};
use time::OffsetDateTime;
use uuid::Uuid;

// ponytail: blocking rusqlite calls inside async trait methods and one shared connection.
// Fine for dev/small load; move to a pool + spawn_blocking if it gets busy.
pub struct SqliteRepo {
    conn: Mutex<Connection>,
}

type UserRow = (String, String, String, String, i64, i64);

impl SqliteRepo {
    /// `path` may be `:memory:` for tests.
    pub fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE IF NOT EXISTS users (
                 id TEXT PRIMARY KEY,
                 name TEXT NOT NULL,
                 email TEXT NOT NULL UNIQUE COLLATE NOCASE,
                 password TEXT NOT NULL,
                 created_at INTEGER NOT NULL,
                 updated_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS sessions (
                 id TEXT PRIMARY KEY,
                 user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                 exp INTEGER NOT NULL,
                 user_agent TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS sessions_exp ON sessions(exp);",
        )?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn find_user(&self, sql: &str, key: &str) -> Result<Option<User>> {
        let row: Option<UserRow> = self
            .conn()
            .query_row(sql, [key], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))
            })
            .optional()?;
        row.map(to_user).transpose()
    }
}

fn to_user((id, name, email, password, created, updated): UserRow) -> Result<User> {
    Ok(User {
        id: Uuid::parse_str(&id)?,
        name,
        email,
        password,
        created_at: OffsetDateTime::from_unix_timestamp(created)?,
        updated_at: OffsetDateTime::from_unix_timestamp(updated)?,
    })
}

fn now_secs() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp()
}

const USER_COLUMNS: &str = "id, name, email, password, created_at, updated_at";

#[async_trait]
impl UsersRepository for SqliteRepo {
    async fn create_user(&self, email: &str, name: &str, hash_password: &str) -> Result<User> {
        let id = Uuid::new_v4();
        let now = now_secs();
        self.conn().execute(
            "INSERT INTO users (id, name, email, password, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            params![id.to_string(), name, email, hash_password, now],
        )?;
        to_user((id.to_string(), name.into(), email.into(), hash_password.into(), now, now))
    }

    async fn get_user_by_email(&self, email: &str) -> Result<Option<User>> {
        self.find_user(&format!("SELECT {USER_COLUMNS} FROM users WHERE email = ?1"), email)
    }

    async fn get_user_by_id(&self, id: Uuid) -> Result<User> {
        self.find_user(&format!("SELECT {USER_COLUMNS} FROM users WHERE id = ?1"), &id.to_string())?
            .ok_or_else(|| anyhow!("user was not found"))
    }

    async fn update_user(&self, id: Uuid, email: String, name: String, password: String) -> Result<User> {
        let changed = self.conn().execute(
            "UPDATE users SET email = ?1, name = ?2, password = ?3, updated_at = ?4 WHERE id = ?5",
            params![email, name, password, now_secs(), id.to_string()],
        )?;
        if changed == 0 {
            return Err(anyhow!("user was not found"));
        }
        self.get_user_by_id(id).await
    }
}

#[async_trait]
impl SessionsRepository for SqliteRepo {
    async fn new_session(&self, user_id: Uuid, user_agent: &str, exp: &OffsetDateTime) -> Result<Session> {
        let id = Uuid::new_v4();
        let c = self.conn();
        c.execute("DELETE FROM sessions WHERE exp < ?1", [now_secs()])?;
        c.execute(
            "INSERT INTO sessions (id, user_id, exp, user_agent) VALUES (?1, ?2, ?3, ?4)",
            params![id.to_string(), user_id.to_string(), exp.unix_timestamp(), user_agent],
        )?;
        Ok(Session { id, user_id, exp: *exp, user_agent: user_agent.into() })
    }

    async fn get_user_by_session_id(&self, id: Uuid) -> Result<Option<User>> {
        self.find_user(
            "SELECT u.id, u.name, u.email, u.password, u.created_at, u.updated_at
             FROM users u JOIN sessions s ON s.user_id = u.id
             WHERE s.id = ?1 AND s.exp >= CAST(strftime('%s', 'now') AS INTEGER)",
            &id.to_string(),
        )
    }
}
