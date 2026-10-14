//! Wires rest_api's users/sessions services, JWT and Argon utils to the SQLite repository.
use crate::sqlite::SqliteRepo;
use anyhow::{Context, Result};
use rest_api::dtos::user_dto::SignUpUserDto;
use rest_api::error::Error;
use rest_api::services::session_services::{DynSessionsService, SessionsService};
use rest_api::services::user_services::{DynUsersService, UsersService};
use rest_api::utils::argon_utils::{ArgonSecurityUtil, DynArgonUtil};
use rest_api::utils::jwt_utils::{DynJwtUtil, JwtTokenUtil};
use rest_api::{AppConfig, CargoEnv};
use std::sync::{Arc, OnceLock};

/// Cookie lifetime for "remember me", matches the 7 days used by SessionsService.
pub const SESSION_TTL_SECS: i64 = 7 * 24 * 3600;

#[derive(Clone)]
pub struct Services {
    pub users: DynUsersService,
    pub sessions: DynSessionsService,
    pub jwt: DynJwtUtil,
}

static SERVICES: OnceLock<Services> = OnceLock::new();

pub fn services() -> &'static Services {
    SERVICES.get().expect("service::init() was not called")
}

pub fn build(config: AppConfig, repo: Arc<SqliteRepo>) -> Services {
    let config = Arc::new(config);
    let argon = Arc::new(ArgonSecurityUtil::new(config.clone())) as DynArgonUtil;
    let jwt = Arc::new(JwtTokenUtil::new(config)) as DynJwtUtil;
    let sessions = Arc::new(SessionsService::new(repo.clone(), jwt.clone())) as DynSessionsService;
    let users = Arc::new(UsersService::new(repo, argon, jwt.clone(), sessions.clone())) as DynUsersService;
    Services { users, sessions, jwt }
}

/// Reads the secrets from the environment (or a .env file), opens SQLite and, when
/// SEED=true, creates the same test users as rest_api's seed service.
pub async fn init() -> Result<()> {
    dotenvy::dotenv().ok();
    let var = |key: &str| std::env::var(key).with_context(|| format!("{key} is not set"));
    let path = std::env::var("SQLITE_PATH").unwrap_or_else(|_| "db_sqlite.db".into());
    let config = AppConfig {
        cargo_env: CargoEnv::Development,
        port: 0,
        database_url: path.clone(),
        run_migrations: false,
        argon_salt: var("ARGON_SALT")?,
        access_token_secret: var("ACCESS_TOKEN_SECRET")?,
        refresh_token_secret: var("REFRESH_TOKEN_SECRET")?,
        cors_origin: String::new(),
        seed: false,
    };
    let services = build(config, Arc::new(SqliteRepo::open(&path)?));
    if std::env::var("SEED").is_ok_and(|v| v == "true") {
        seed(&services).await?;
    }
    SERVICES.set(services).map_err(|_| anyhow::anyhow!("service::init() called twice"))
}

async fn seed(services: &Services) -> Result<()> {
    for n in 1..=3 {
        let dto = SignUpUserDto {
            name: Some(format!("testuser{n}")),
            email: Some(format!("testuser{n}@gmail.com")),
            password: Some("password".into()),
        };
        match services.users.signup_user(dto).await {
            Ok(_) | Err(Error::ObjectConflict(_)) => {}
            Err(e) => return Err(anyhow::anyhow!("seeding testuser{n} failed: {e}")),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rest_api::database::session::SessionsRepository;
    use rest_api::dtos::user_dto::SignInUserDto;
    use time::OffsetDateTime;
    use uuid::Uuid;

    fn test_services() -> (Services, Arc<SqliteRepo>) {
        let repo = Arc::new(SqliteRepo::open(":memory:").unwrap());
        let config = AppConfig {
            cargo_env: CargoEnv::Development,
            port: 0,
            database_url: ":memory:".into(),
            run_migrations: false,
            argon_salt: "test-salt-long-enough".into(),
            access_token_secret: "test-access-secret".into(),
            refresh_token_secret: "test-refresh-secret".into(),
            cors_origin: String::new(),
            seed: false,
        };
        (build(config, repo.clone()), repo)
    }

    fn signup(email: &str) -> SignUpUserDto {
        SignUpUserDto { name: Some("Budi".into()), email: Some(email.into()), password: Some("secret1".into()) }
    }

    fn signin(email: &str, password: &str) -> SignInUserDto {
        SignInUserDto { email: Some(email.into()), password: Some(password.into()) }
    }

    #[tokio::test]
    async fn signup_signin_and_refresh_through_sqlite() {
        let (s, _) = test_services();
        s.users.signup_user(signup("budi@example.com")).await.unwrap();
        let (user, refresh) = s
            .users
            .signin_user(signin("BUDI@example.com", "secret1"), Some("test".into()))
            .await
            .unwrap();
        assert_eq!((user.name.as_str(), user.email.as_str()), ("Budi", "budi@example.com"));

        let session_id = s.jwt.get_session_id_from_token(refresh).unwrap();
        let me = s.sessions.refresh_access_token(session_id).await.unwrap();
        assert_eq!(me.email, "budi@example.com");
    }

    #[tokio::test]
    async fn rejects_duplicate_email_and_wrong_password() {
        let (s, _) = test_services();
        s.users.signup_user(signup("a@example.com")).await.unwrap();
        assert!(matches!(s.users.signup_user(signup("A@example.com")).await, Err(Error::ObjectConflict(_))));
        assert!(matches!(
            s.users.signin_user(signin("a@example.com", "wrong-pass"), Some("t".into())).await,
            Err(Error::InvalidLoginAttmpt)
        ));
        assert!(matches!(
            s.users.signin_user(signin("nobody@example.com", "secret1"), Some("t".into())).await,
            Err(Error::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn expired_or_unknown_session_is_unauthorized() {
        let (s, repo) = test_services();
        let user = s.users.signup_user(signup("c@example.com")).await.unwrap();
        let past = OffsetDateTime::now_utc() - time::Duration::hours(1);
        let expired = repo.new_session(user.id, "t", &past).await.unwrap();
        assert!(matches!(s.sessions.refresh_access_token(expired.id).await, Err(Error::Unauthorized)));
        assert!(matches!(s.sessions.refresh_access_token(Uuid::new_v4()).await, Err(Error::Unauthorized)));
    }
}
