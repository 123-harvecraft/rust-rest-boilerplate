//! Single page ported from the harvecraft HTML: navbar, login/register forms and the
//! dashboard shell. Class names match `assets/styles.css` (the original stylesheet).
use crate::api::{login, logout, profile, register, Profile};
use dioxus::prelude::*;
use std::sync::atomic::{AtomicU64, Ordering};

const AVATAR: &str = "https://i.pinimg.com/736x/e2/7c/87/e27c8735da98ec6ccdcf12e258b26475.jpg";

#[derive(Clone, PartialEq)]
struct Toast {
    id: u64,
    message: String,
    kind: &'static str,
}

#[derive(Clone, Copy, PartialEq)]
enum Guest {
    Login,
    Register,
}

/// Shared page state, provided once by `App`.
#[derive(Clone, Copy)]
struct Ctx {
    me: Resource<Option<Profile>>,
    guest: Signal<Guest>,
    toast: Signal<Option<Toast>>,
    /// Email to pre-fill on the login form after a successful registration.
    prefill: Signal<String>,
}

static TOAST_SEQ: AtomicU64 = AtomicU64::new(0);

async fn sleep_ms(ms: u32) {
    #[cfg(feature = "web")]
    gloo_timers::future::TimeoutFuture::new(ms).await;
    #[cfg(not(feature = "web"))]
    let _ = ms;
}

fn show_toast(mut toast: Signal<Option<Toast>>, message: impl Into<String>, kind: &'static str) {
    let id = TOAST_SEQ.fetch_add(1, Ordering::Relaxed);
    toast.set(Some(Toast { id, message: message.into(), kind }));
    spawn(async move {
        sleep_ms(4000).await;
        if toast.peek().as_ref().is_some_and(|t| t.id == id) {
            toast.set(None);
        }
    });
}

fn error_message(err: ServerFnError) -> String {
    match err {
        ServerFnError::ServerError { message, .. } => message,
        other => other.to_string(),
    }
}

#[component]
pub fn App() -> Element {
    let me = use_resource(|| async { profile().await.ok().flatten() });
    let ctx = use_context_provider(|| Ctx {
        me,
        guest: Signal::new(Guest::Login),
        toast: Signal::new(None),
        prefill: Signal::new(String::new()),
    });
    let user = (*me.read_unchecked()).clone();

    rsx! {
        document::Stylesheet { href: "https://cdnjs.cloudflare.com/ajax/libs/font-awesome/6.0.0/css/all.min.css" }
        document::Stylesheet { href: asset!("/assets/styles.css") }
        Navbar {}
        div { class: "container",
            match user {
                None => rsx! { p { "Loading..." } },
                Some(Some(p)) => rsx! { Dashboard { user: p } },
                Some(None) => rsx! {
                    if (ctx.guest)() == Guest::Login { LoginForm {} } else { RegisterForm {} }
                },
            }
        }
        ToastView {}
    }
}

#[component]
fn Navbar() -> Element {
    let mut ctx = use_context::<Ctx>();
    let mut menu_open = use_signal(|| false);
    let logged_in = matches!(&*ctx.me.read_unchecked(), Some(Some(_)));

    rsx! {
        nav { class: "navbar", id: "navbar",
            div { class: "nav-container",
                div { class: "nav-logo",
                    i { class: "fas fa-shield-alt" }
                    span { "CoMate" }
                }
                div { class: if menu_open() { "nav-menu active" } else { "nav-menu" },
                    if logged_in {
                        a {
                            href: "#",
                            class: "nav-link",
                            onclick: move |e| {
                                e.prevent_default();
                                spawn(async move {
                                    let _ = logout().await;
                                    ctx.guest.set(Guest::Login);
                                    ctx.me.restart();
                                    show_toast(ctx.toast, "Logged out", "success");
                                });
                            },
                            "Logout"
                        }
                    } else {
                        a {
                            href: "#",
                            class: "nav-link",
                            onclick: move |e| {
                                e.prevent_default();
                                ctx.guest.set(Guest::Login);
                            },
                            "Login"
                        }
                        a {
                            href: "#",
                            class: "nav-link",
                            onclick: move |e| {
                                e.prevent_default();
                                ctx.guest.set(Guest::Register);
                            },
                            "Register"
                        }
                    }
                }
                div {
                    class: if menu_open() { "hamburger active" } else { "hamburger" },
                    onclick: move |_| menu_open.set(!menu_open()),
                    span { class: "bar" }
                    span { class: "bar" }
                    span { class: "bar" }
                }
            }
        }
    }
}

#[component]
fn LoginForm() -> Element {
    let mut ctx = use_context::<Ctx>();
    let mut email = use_signal(move || ctx.prefill.peek().clone());
    let mut password = use_signal(String::new);
    let mut remember = use_signal(|| false);
    let mut show_pw = use_signal(|| false);
    let mut busy = use_signal(|| false);

    rsx! {
        div { class: "form-container", id: "login-form",
            div { class: "form-header",
                h2 {
                    i { class: "fas fa-sign-in-alt" }
                    " Welcome Back!"
                }
                p { "Sign in to your account" }
            }
            form {
                class: "auth-form",
                onsubmit: move |e| async move {
                    e.prevent_default();
                    busy.set(true);
                    match login(email(), password(), remember()).await {
                        Ok(p) => {
                            password.set(String::new());
                            show_toast(ctx.toast, format!("Welcome {}!", p.name), "success");
                            ctx.me.restart();
                        }
                        Err(err) => show_toast(ctx.toast, error_message(err), "error"),
                    }
                    busy.set(false);
                },
                div { class: "input-group",
                    label { r#for: "login-email", "Email" }
                    div { class: "input-wrapper",
                        i { class: "fas fa-envelope" }
                        input {
                            r#type: "email",
                            id: "login-email",
                            placeholder: "Enter your email",
                            required: true,
                            value: "{email}",
                            oninput: move |e| email.set(e.value()),
                        }
                    }
                }
                div { class: "input-group",
                    label { r#for: "login-password", "Password" }
                    div { class: "input-wrapper",
                        i { class: "fas fa-lock" }
                        input {
                            r#type: if show_pw() { "text" } else { "password" },
                            id: "login-password",
                            placeholder: "Enter your password",
                            required: true,
                            value: "{password}",
                            oninput: move |e| password.set(e.value()),
                        }
                        i {
                            class: if show_pw() { "fas fa-eye-slash toggle-password" } else { "fas fa-eye toggle-password" },
                            onclick: move |_| show_pw.set(!show_pw()),
                        }
                    }
                }
                div { class: "form-options",
                    label { class: "checkbox-container",
                        input {
                            r#type: "checkbox",
                            id: "remember-me",
                            checked: remember(),
                            onchange: move |e| remember.set(e.checked()),
                        }
                        span { class: "checkmark" }
                        "Remember me"
                    }
                }
                button { r#type: "submit", class: "btn btn-primary", disabled: busy(),
                    i { class: "fas fa-sign-in-alt" }
                    " Sign In"
                }
                div { class: "form-footer",
                    p {
                        "Don't have an account? "
                        a {
                            href: "#",
                            onclick: move |e| {
                                e.prevent_default();
                                ctx.guest.set(Guest::Register);
                            },
                            "Create one"
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn RegisterForm() -> Element {
    let mut ctx = use_context::<Ctx>();
    let mut username = use_signal(String::new);
    let mut email = use_signal(String::new);
    let mut password = use_signal(String::new);
    let mut confirm = use_signal(String::new);
    let mut agree = use_signal(|| false);
    let mut show_pw = use_signal(|| false);
    let mut busy = use_signal(|| false);

    rsx! {
        div { class: "form-container", id: "register-form",
            div { class: "form-header",
                h2 {
                    i { class: "fas fa-user-plus" }
                    " Create Account"
                }
                p { "Join us today" }
            }
            form {
                class: "auth-form",
                onsubmit: move |e| async move {
                    e.prevent_default();
                    if password() != confirm() {
                        show_toast(ctx.toast, "Passwords do not match", "error");
                        return;
                    }
                    if password().len() < 6 {
                        show_toast(ctx.toast, "Password must be at least 6 characters", "error");
                        return;
                    }
                    busy.set(true);
                    match register(username(), email(), password()).await {
                        Ok(()) => {
                            show_toast(ctx.toast, format!("Registration successful! Welcome {}!", username().trim()), "success");
                            ctx.prefill.set(email().trim().to_string());
                            ctx.guest.set(Guest::Login);
                        }
                        Err(err) => show_toast(ctx.toast, error_message(err), "error"),
                    }
                    busy.set(false);
                },
                div { class: "input-group",
                    label { r#for: "register-username", "Username" }
                    div { class: "input-wrapper",
                        i { class: "fas fa-user" }
                        input {
                            r#type: "text",
                            id: "register-username",
                            placeholder: "Choose a username",
                            required: true,
                            value: "{username}",
                            oninput: move |e| username.set(e.value()),
                        }
                    }
                }
                div { class: "input-group",
                    label { r#for: "register-email", "Email" }
                    div { class: "input-wrapper",
                        i { class: "fas fa-envelope" }
                        input {
                            r#type: "email",
                            id: "register-email",
                            placeholder: "Enter your email",
                            required: true,
                            value: "{email}",
                            oninput: move |e| email.set(e.value()),
                        }
                    }
                }
                div { class: "input-group",
                    label { r#for: "register-password", "Password" }
                    div { class: "input-wrapper",
                        i { class: "fas fa-lock" }
                        input {
                            r#type: if show_pw() { "text" } else { "password" },
                            id: "register-password",
                            placeholder: "Create a password",
                            required: true,
                            value: "{password}",
                            oninput: move |e| password.set(e.value()),
                        }
                        i {
                            class: if show_pw() { "fas fa-eye-slash toggle-password" } else { "fas fa-eye toggle-password" },
                            onclick: move |_| show_pw.set(!show_pw()),
                        }
                    }
                }
                div { class: "input-group",
                    label { r#for: "register-confirm-password", "Confirm Password" }
                    div { class: "input-wrapper",
                        i { class: "fas fa-lock" }
                        input {
                            r#type: if show_pw() { "text" } else { "password" },
                            id: "register-confirm-password",
                            placeholder: "Confirm your password",
                            required: true,
                            value: "{confirm}",
                            oninput: move |e| confirm.set(e.value()),
                        }
                    }
                }
                div { class: "form-options",
                    label { class: "checkbox-container",
                        input {
                            r#type: "checkbox",
                            id: "agree-terms",
                            required: true,
                            checked: agree(),
                            onchange: move |e| agree.set(e.checked()),
                        }
                        span { class: "checkmark" }
                        "I agree to the Terms & Conditions"
                    }
                }
                button { r#type: "submit", class: "btn btn-primary", disabled: busy(),
                    i { class: "fas fa-user-plus" }
                    " Create Account"
                }
                div { class: "form-footer",
                    p {
                        "Already have an account? "
                        a {
                            href: "#",
                            onclick: move |e| {
                                e.prevent_default();
                                ctx.guest.set(Guest::Login);
                            },
                            "Sign in"
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn Dashboard(user: Profile) -> Element {
    let mut collapsed = use_signal(|| false);

    rsx! {
        div { class: "dashboard-container", id: "dashboard",
            div { class: "dashboard-header-bar",
                div { class: "header-left",
                    button { class: "sidebar-toggle", onclick: move |_| collapsed.set(!collapsed()),
                        i { class: "fas fa-bars" }
                    }
                    h1 { class: "page-title", "Profile" }
                }
            }
            div { class: "dashboard-layout",
                // `collapsed` hides it on desktop, `show` reveals it on narrow screens (<=1024px).
                div { class: if collapsed() { "sidebar collapsed show" } else { "sidebar" },
                    div { class: "sidebar-header",
                        div { class: "sidebar-logo",
                            img { src: AVATAR, alt: "Avatar", class: "user-avatar-logo" }
                            div { class: "user-info-header",
                                span { class: "user-name-header", "{user.name}" }
                                span { class: "user-title", "{user.email}" }
                            }
                        }
                    }
                    nav { class: "sidebar-nav",
                        ul { class: "nav-list",
                            li { class: "nav-item active",
                                a { href: "#", class: "nav-link", onclick: move |e| e.prevent_default(),
                                    i { class: "fas fa-user" }
                                    span { "Profile" }
                                }
                            }
                        }
                    }
                }
                div { class: "main-content",
                    div { class: "page-content active",
                        div { class: "content-header",
                            h2 { "Profile" }
                        }
                        p { "Name: {user.name}" }
                        p { "Email: {user.email}" }
                    }
                }
            }
        }
    }
}

#[component]
fn ToastView() -> Element {
    let mut ctx = use_context::<Ctx>();
    let current = ctx.toast.read().clone();
    let (class, message) = match &current {
        Some(t) => (format!("toast show {}", t.kind), t.message.clone()),
        None => ("toast".to_string(), String::new()),
    };

    rsx! {
        div { class: "{class}", id: "toast",
            div { class: "toast-content",
                span { class: "toast-message", "{message}" }
                button { class: "toast-close", onclick: move |_| ctx.toast.set(None),
                    i { class: "fas fa-times" }
                }
            }
        }
    }
}
