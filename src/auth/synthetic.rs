//! Synthetic filesystem control surfaces for authentication.

use anyhow::{anyhow, Context, Result};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::synth::{ControlHandler, SyntheticFilesystem};

use super::{AuthService, Capability, SessionToken};

/// Register authentication control files under /srv/auth
pub async fn register_auth_controls(
    synth_fs: &Arc<SyntheticFilesystem>,
    auth: Arc<AuthService>,
) -> Result<()> {
    let base = std::path::Path::new("/srv/auth");
    synth_fs.create_directory(base).await?;

    synth_fs
        .create_control_file(
            &base.join("login"),
            Arc::new(LoginControl::new(auth.clone())),
        )
        .await?;

    synth_fs
        .create_control_file(
            &base.join("logout"),
            Arc::new(LogoutControl::new(auth.clone())),
        )
        .await?;

    synth_fs
        .create_control_file(
            &base.join("create"),
            Arc::new(CreateUserControl::new(auth.clone())),
        )
        .await?;

    synth_fs
        .create_control_file(
            &base.join("delete"),
            Arc::new(DeleteUserControl::new(auth.clone())),
        )
        .await?;

    synth_fs
        .create_control_file(&base.join("users"), Arc::new(ListUsersControl::new(auth)))
        .await?;

    Ok(())
}

/// Helper to execute async work within control handlers.
fn block_on_async<F, T>(future: F) -> Result<T>
where
    F: std::future::Future<Output = Result<T>> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(future))
}

struct LoginControl {
    auth: Arc<AuthService>,
    last_response: Arc<RwLock<Option<Vec<u8>>>>,
}

impl LoginControl {
    fn new(auth: Arc<AuthService>) -> Self {
        Self {
            auth,
            last_response: Arc::new(RwLock::new(None)),
        }
    }
}

impl ControlHandler for LoginControl {
    fn read(&self) -> Result<Vec<u8>> {
        block_on_async({
            let last = self.last_response.clone();
            async move {
                let mut guard = last.write().await;
                if let Some(response) = guard.take() {
                    Ok(response)
                } else {
                    Ok(b"write \"username password\" to receive a session token\n".to_vec())
                }
            }
        })
    }

    fn write(&self, data: &[u8]) -> Result<()> {
        let payload = String::from_utf8(data.to_vec())
            .map_err(|_| anyhow!("Credentials must be valid UTF-8"))?;

        let mut parts = payload.split_whitespace();
        let username = parts.next().ok_or_else(|| anyhow!("Missing username"))?;
        let password = parts.next().ok_or_else(|| anyhow!("Missing password"))?;

        block_on_async({
            let auth = self.auth.clone();
            let username = username.to_string();
            let password = password.to_string();
            let last = self.last_response.clone();
            async move {
                match auth.authenticate(&username, &password, None).await {
                    Ok(token) => {
                        let mut guard = last.write().await;
                        guard.replace(format!("{}\n", token.as_str()).into_bytes());
                        Ok(())
                    }
                    Err(err) => Err(anyhow!(err)),
                }
            }
        })
    }
}

struct LogoutControl {
    auth: Arc<AuthService>,
}

impl LogoutControl {
    fn new(auth: Arc<AuthService>) -> Self {
        Self { auth }
    }
}

impl ControlHandler for LogoutControl {
    fn read(&self) -> Result<Vec<u8>> {
        Ok(b"write session token to revoke\n".to_vec())
    }

    fn write(&self, data: &[u8]) -> Result<()> {
        let token_str = String::from_utf8(data.to_vec())
            .map_err(|_| anyhow!("Token must be UTF-8"))?
            .trim()
            .to_string();

        if token_str.is_empty() {
            anyhow::bail!("Token cannot be empty");
        }

        block_on_async({
            let auth = self.auth.clone();
            async move {
                let token = SessionToken::from_string(token_str);
                auth.logout(&token).await?;
                Ok(())
            }
        })
    }
}

struct CreateUserControl {
    auth: Arc<AuthService>,
}

impl CreateUserControl {
    fn new(auth: Arc<AuthService>) -> Self {
        Self { auth }
    }
}

impl ControlHandler for CreateUserControl {
    fn read(&self) -> Result<Vec<u8>> {
        Ok(b"write: username password uid gid [capabilities]\n".to_vec())
    }

    fn write(&self, data: &[u8]) -> Result<()> {
        let payload =
            String::from_utf8(data.to_vec()).map_err(|_| anyhow!("Input must be UTF-8"))?;

        let mut parts = payload.split_whitespace();
        let username = parts.next().ok_or_else(|| anyhow!("Missing username"))?;
        let password = parts.next().ok_or_else(|| anyhow!("Missing password"))?;
        let uid = parts
            .next()
            .ok_or_else(|| anyhow!("Missing uid"))?
            .parse::<u32>()
            .context("uid must be numeric")?;
        let gid = parts
            .next()
            .ok_or_else(|| anyhow!("Missing gid"))?
            .parse::<u32>()
            .context("gid must be numeric")?;

        // Remaining tokens can be either a single comma-separated list or space separated
        let mut capabilities: Vec<Capability> = Vec::new();
        let remainder: String = parts.collect::<Vec<_>>().join(" ");
        if !remainder.is_empty() {
            let tokens = remainder.split(|c| c == ',' || c == ' ');
            for token in tokens.map(|t| t.trim()).filter(|t| !t.is_empty()) {
                capabilities.push(parse_capability(token));
            }
        }

        if capabilities.is_empty() {
            capabilities.push(Capability::Read);
        }

        block_on_async({
            let auth = self.auth.clone();
            let username = username.to_string();
            let password = password.to_string();
            async move {
                auth.create_user(&username, &password, uid, gid, capabilities)
                    .await?;
                Ok(())
            }
        })
    }
}

struct DeleteUserControl {
    auth: Arc<AuthService>,
}

impl DeleteUserControl {
    fn new(auth: Arc<AuthService>) -> Self {
        Self { auth }
    }
}

impl ControlHandler for DeleteUserControl {
    fn read(&self) -> Result<Vec<u8>> {
        Ok(b"write username to delete\n".to_vec())
    }

    fn write(&self, data: &[u8]) -> Result<()> {
        let username = String::from_utf8(data.to_vec())
            .map_err(|_| anyhow!("Username must be UTF-8"))?
            .trim()
            .to_string();

        if username.is_empty() {
            anyhow::bail!("Username cannot be empty");
        }

        block_on_async({
            let auth = self.auth.clone();
            async move {
                auth.delete_user(&username).await?;
                Ok(())
            }
        })
    }
}

struct ListUsersControl {
    auth: Arc<AuthService>,
}

impl ListUsersControl {
    fn new(auth: Arc<AuthService>) -> Self {
        Self { auth }
    }
}

impl ControlHandler for ListUsersControl {
    fn read(&self) -> Result<Vec<u8>> {
        block_on_async({
            let auth = self.auth.clone();
            async move {
                let users = auth.list_users().await;
                Ok(format!("{}\n", users.join("\n")).into_bytes())
            }
        })
    }

    fn write(&self, _data: &[u8]) -> Result<()> {
        Ok(()) // read-only
    }
}

fn parse_capability(token: &str) -> Capability {
    match token.to_lowercase().as_str() {
        "read" => Capability::Read,
        "write" => Capability::Write,
        "execute" | "exec" => Capability::Execute,
        "mount" => Capability::Mount,
        "admin" => Capability::Admin,
        "translator" | "create_translator" => Capability::CreateTranslator,
        "mesh" | "mesh_access" => Capability::MeshAccess,
        other => {
            if let Some(custom) = other.strip_prefix("custom:") {
                Capability::Custom(custom.to_string())
            } else {
                Capability::Custom(other.to_string())
            }
        }
    }
}
