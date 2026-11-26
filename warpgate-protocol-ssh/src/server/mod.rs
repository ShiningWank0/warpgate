mod channel_registry;
mod channel_writer;
pub(crate) mod client_closer;
mod event_intake;
mod russh_handler;
mod service_output;
mod session;
mod session_deadline;
mod session_handle;
mod target_menu;
use std::borrow::Cow;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use futures::FutureExt;
use futures::future::BoxFuture;
use russh::keys::{Algorithm, HashAlg, PrivateKey};
use russh::{MethodKind, MethodSet, Preferred};
pub use russh_handler::ServerHandler;
pub use session::ServerSession;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::sync::mpsc::unbounded_channel;
use tokio::time::Instant;
use tracing::*;
use warpgate_common::helpers::net::accept_loop;
use warpgate_common::{ListenEndpoint, SshConfig};
use warpgate_core::{Services, State, UserSessionStateInit};
use warpgate_db_entities::Parameters;

use crate::keys::load_host_keys;
use crate::server::client_closer::ClientCloser;
use crate::server::session_handle::SSHSessionHandle;

#[derive(Clone)]
struct RusshConfigInit {
    keys: Vec<PrivateKey>,
}

pub async fn bind_server(
    services: Services,
    address: ListenEndpoint,
    proxy_protocol: bool,
) -> Result<BoxFuture<'static, Result<()>>> {
    read_ssh_config(&services).await?;
    let russh_config_init = Arc::new({
        RusshConfigInit {
            keys: load_host_keys(&services.db).await?,
        }
    });

    let listener = address.tcp_accept_stream().await?;

    Ok(async move {
        accept_loop(
            "SSH connection",
            listener,
            proxy_protocol,
            move |stream, remote_address| {
                let russh_config_init = russh_config_init.clone();
                let services = services.clone();
                async move {
                    _handle_connection(services, russh_config_init, stream, remote_address).await
                }
            },
        )
        .await;
        Ok(())
    }
    .boxed())
}

async fn read_ssh_config(services: &Services) -> Result<SshConfig> {
    let ssh = services.config.lock().await.store.ssh.clone();
    if let Some(ttl) = ssh.ticket_max_ttl {
        anyhow::ensure!(
            !ttl.is_zero()
                && ssh
                    .session_open_timeout
                    .is_some_and(|duration| !duration.is_zero())
                && ssh
                    .session_max_age
                    .is_some_and(|duration| !duration.is_zero()),
            "SSH ticket_max_ttl requires positive session_open_timeout and session_max_age"
        );
    }
    let now = Instant::now();
    anyhow::ensure!(
        [ssh.session_open_timeout, ssh.session_max_age]
            .into_iter()
            .flatten()
            .all(|duration| now.checked_add(duration).is_some()),
        "SSH session deadline is too large"
    );
    Ok(ssh)
}

pub(super) struct SessionSettings {
    accepted_at: Instant,
    ssh: SshConfig,
    allowed_auth_methods: MethodSet,
}

async fn _handle_connection(
    services: Services,
    russh_config_init: Arc<RusshConfigInit>,
    stream: TcpStream,
    remote_address: SocketAddr,
) -> Result<()> {
    let accepted_at = Instant::now();
    let ssh = read_ssh_config(&services).await?;
    let methods = get_allowed_auth_methods(&services, ssh.ticket_max_ttl.is_some()).await?;
    let (session_handle, session_handle_rx) = SSHSessionHandle::new();

    let client_closer = ClientCloser::default();

    let (server_handle, wrapped_stream) = State::register_user_session_with_stream(
        &services.state,
        crate::PROTOCOL_NAME,
        UserSessionStateInit {
            remote_address: Some(remote_address),
            handle: Box::new(session_handle),
        },
        stream,
    )
    .await
    .context("registering session")?;
    // The guard is outermost. Closing must also release a write that the
    // rate limiter is delaying
    let wrapped_stream = client_closer.guard(wrapped_stream);

    let id = server_handle.lock().await.user_session_id();

    let (event_tx, event_rx) = unbounded_channel();

    let banner = {
        let db = &services.db;
        // Normalize line endings for terminal display.
        Parameters::Entity::get(db)
            .await?
            .banner_text()
            .map(|text| format!("{}\r\n", text.replace("\r\n", "\n").replace('\n', "\r\n")))
    };

    let handler = ServerHandler { event_tx, banner };

    let session = match ServerSession::start(
        remote_address,
        &services,
        server_handle,
        session_handle_rx,
        event_rx,
        client_closer,
        SessionSettings {
            accepted_at,
            ssh: ssh.clone(),
            allowed_auth_methods: methods.clone(),
        },
    )
    .await
    {
        Ok(session) => session,
        Err(error) => {
            error!(%error, "Error setting up session");
            return Err(error);
        }
    };

    let russh_config = {
        russh::server::Config {
            auth_rejection_time: Duration::from_secs(1),
            auth_rejection_time_initial: Some(Duration::from_secs(0)),
            inactivity_timeout: Some(
                ssh.inactivity_timeout
                // There is no traffic during admin approval hold that would
                // reset the inactivity timer, so the timeout needs to be at least that long
                + services.admin_approval_timeout().await?
                // Extra time for the "closing due to inactivity" message to be sent
                + Duration::from_secs(10),
            ),
            keepalive_interval: ssh.keepalive_interval,
            methods,
            keys: russh_config_init.keys.clone(),
            event_buffer_size: 100,
            nodelay: true,
            preferred: Preferred {
                key: Cow::Borrowed(&[
                    Algorithm::Ed25519,
                    Algorithm::Rsa {
                        hash: Some(HashAlg::Sha512),
                    },
                    Algorithm::Rsa {
                        hash: Some(HashAlg::Sha256),
                    },
                    Algorithm::Rsa { hash: None },
                ]),
                ..<_>::default()
            },
            ..<_>::default()
        }
    };

    let russh_config = Arc::new(russh_config);

    tokio::task::Builder::new()
        .name(&format!("SSH {id} session"))
        .spawn(session)?;

    tokio::task::Builder::new()
        .name(&format!("SSH {id} protocol"))
        .spawn(_run_stream(russh_config, wrapped_stream, handler))?;

    Ok(())
}

async fn _run_stream<R>(
    config: Arc<russh::server::Config>,
    socket: R,
    handler: ServerHandler,
) -> Result<()>
where
    R: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let ret = async move {
        let session = russh::server::run_stream(config, socket, handler).await?;
        session.await?;
        Ok(())
    }
    .await;

    if let Err(ref error) = ret {
        error!(%error, "Session failed");
    }

    ret
}

async fn get_allowed_auth_methods(services: &Services, ticket_only: bool) -> Result<MethodSet> {
    if ticket_only {
        return Ok(MethodSet::from(
            &[MethodKind::Password, MethodKind::KeyboardInteractive][..],
        ));
    }
    let parameters = {
        let db = &services.db;
        Parameters::Entity::get(db).await?
    };

    let mut methods_vec: Vec<MethodKind> = Vec::new();
    if parameters.ssh_client_auth_publickey {
        methods_vec.push(MethodKind::PublicKey);
    }
    if parameters.ssh_client_auth_password {
        methods_vec.push(MethodKind::Password);
    }
    if parameters.ssh_client_auth_keyboard_interactive {
        methods_vec.push(MethodKind::KeyboardInteractive);
    }

    if methods_vec.is_empty() {
        warn!(
            "All SSH authentication methods are disabled in parameters. Enabling all methods as fallback."
        );
        methods_vec = vec![
            MethodKind::PublicKey,
            MethodKind::Password,
            MethodKind::KeyboardInteractive,
        ];
    }

    Ok(MethodSet::from(&methods_vec[..]))
}
