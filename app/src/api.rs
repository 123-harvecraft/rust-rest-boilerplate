//! Server functions: the only entry points the page can call. They delegate to rest_api's services.
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    pub email: String,
}

#[cfg(feature = "server")]
fn server_error(code: u16, message: &str) -> ServerFnError {
    ServerFnError::ServerError { message: message.into(), code, details: None }
}

/// Maps a rest_api error to a safe message. Internal details stay in the server log.
#[cfg(feature = "server")]
fn map_error(e: rest_api::error::Error) -> ServerFnError {
    use rest_api::error::Error;
    match e {
        Error::NotFound(_) | Error::InvalidLoginAttmpt | Error::Unauthorized => server_error(401, "Invalid credentials"),
        Error::ObjectConflict(_) => server_error(409, "Email already registered"),
        other => {
            eprintln!("auth error: {other}");
            server_error(500, "Something went wrong")
        }
    }
}

#[cfg(feature = "server")]
fn refresh_token(headers: &dioxus::fullstack::HeaderMap) -> Option<String> {
    headers
        .get("cookie")?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|c| c.trim().strip_prefix("refresh_token="))
        .map(str::to_owned)
}

// ponytail: no Secure flag, local http dev. Add `; Secure` behind https.
// `max_age: None` makes a browser-session cookie ("remember me" unchecked).
#[cfg(feature = "server")]
fn set_refresh_cookie(value: &str, max_age: Option<i64>) -> Result<(), ServerFnError> {
    use dioxus::fullstack::{http::header::SET_COOKIE, FullstackContext, HeaderValue};
    let mut cookie = format!("refresh_token={value}; HttpOnly; SameSite=Lax; Path=/");
    if let Some(secs) = max_age {
        cookie.push_str(&format!("; Max-Age={secs}"));
    }
    let ctx = FullstackContext::current().ok_or_else(|| ServerFnError::new("no request context"))?;
    ctx.add_response_header(SET_COOKIE, HeaderValue::from_str(&cookie).map_err(ServerFnError::new)?);
    Ok(())
}

#[post("/api/login", headers: dioxus::fullstack::HeaderMap)]
pub async fn login(email: String, password: String, remember: bool) -> Result<Profile, ServerFnError> {
    use rest_api::dtos::user_dto::SignInUserDto;
    if email.len() > 254 || password.len() > 1024 {
        return Err(server_error(401, "Invalid credentials"));
    }
    let user_agent: String = headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown")
        .chars()
        .take(255)
        .collect();
    let dto = SignInUserDto { email: Some(email.trim().to_string()), password: Some(password) };
    let (user, refresh) = crate::service::services()
        .users
        .signin_user(dto, Some(user_agent))
        .await
        .map_err(map_error)?;
    set_refresh_cookie(&refresh, remember.then_some(crate::service::SESSION_TTL_SECS))?;
    Ok(Profile { name: user.name, email: user.email })
}

#[post("/api/register")]
pub async fn register(name: String, email: String, password: String) -> Result<(), ServerFnError> {
    use rest_api::dtos::user_dto::SignUpUserDto;
    let (name, email) = (name.trim(), email.trim());
    if name.is_empty() || name.chars().count() > 64 || name.chars().any(char::is_control) {
        return Err(server_error(400, "Username must be 1-64 characters"));
    }
    if email.len() > 254 || !email.contains('@') {
        return Err(server_error(400, "Enter a valid email"));
    }
    if password.len() < 6 || password.len() > 1024 {
        return Err(server_error(400, "Password must be at least 6 characters"));
    }
    let dto = SignUpUserDto { name: Some(name.into()), email: Some(email.into()), password: Some(password) };
    crate::service::services().users.signup_user(dto).await.map_err(map_error)?;
    Ok(())
}

/// `None` means no valid session (not an error).
#[get("/api/profile", headers: dioxus::fullstack::HeaderMap)]
pub async fn profile() -> Result<Option<Profile>, ServerFnError> {
    use rest_api::error::Error;
    let Some(token) = refresh_token(&headers) else { return Ok(None) };
    let s = crate::service::services();
    let Ok(session_id) = s.jwt.get_session_id_from_token(token) else { return Ok(None) };
    match s.sessions.refresh_access_token(session_id).await {
        Ok(user) => Ok(Some(Profile { name: user.name, email: user.email })),
        Err(Error::Unauthorized) => Ok(None),
        Err(e) => Err(map_error(e)),
    }
}

// Like rest_api's signout, this only clears the cookie: SessionsRepository has no delete.
#[post("/api/logout")]
pub async fn logout() -> Result<(), ServerFnError> {
    set_refresh_cookie("", Some(0))
}
