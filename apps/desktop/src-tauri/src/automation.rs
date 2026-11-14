//! The automation API inside the desktop app (M28): started at launch when
//! it is enabled in `api.toml`, restarted when its settings or token change.

use chosproxy_api::{ApiServer, token};
use chosproxy_app::{ApiStatus, Engine};
use tokio::sync::Mutex;

/// The running server, if any.
#[derive(Default)]
pub(crate) struct Automation {
    server: Mutex<Option<ApiServer>>,
    error: Mutex<Option<String>>,
}

impl Automation {
    /// Stops the server and starts it again if enabled.
    pub(crate) async fn restart(&self, engine: &Engine) -> ApiStatus {
        let mut server = self.server.lock().await;
        if let Some(old) = server.take() {
            old.stop().await;
        }
        let config = engine.api_config();
        let mut error = None;
        if config.enabled {
            match token::load_or_create(&engine.paths().data_dir) {
                Ok(t) => match ApiServer::start(engine.clone(), config.port, t).await {
                    Ok(s) => *server = Some(s),
                    Err(e) => {
                        error = Some(format!("cannot listen on 127.0.0.1:{}: {e}", config.port))
                    }
                },
                Err(e) => error = Some(format!("cannot create the token: {e}")),
            }
        }
        *self.error.lock().await = error;
        drop(server);
        self.status(engine).await
    }

    /// Where the API stands.
    pub(crate) async fn status(&self, engine: &Engine) -> ApiStatus {
        let server = self.server.lock().await;
        let data_dir = &engine.paths().data_dir;
        ApiStatus {
            config: engine.api_config(),
            running: server.is_some(),
            address: server.as_ref().map(|s| s.addr().to_string()),
            token: token::read(data_dir).unwrap_or_default(),
            token_file: token::token_path(data_dir).display().to_string(),
            error: self.error.lock().await.clone(),
        }
    }
}
