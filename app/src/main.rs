mod api;
mod ui;

#[cfg(feature = "server")]
mod service;
#[cfg(feature = "server")]
mod sqlite;

fn main() {
    #[cfg(not(feature = "server"))]
    dioxus::launch(ui::App);

    #[cfg(feature = "server")]
    dioxus::serve(|| async {
        service::init().await?;
        Ok(dioxus::server::router(ui::App))
    });
}
