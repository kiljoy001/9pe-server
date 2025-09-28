//! 9P.e Server - Clean Architecture Implementation
//!
//! This is the refactored version with proper separation of concerns,
//! dependency injection, and modern Rust patterns.

use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use tracing::{info, error, Level};
use tracing_subscriber::FmtSubscriber;

use ninep_server::{
    cli::Cli,
    server::Server,
    network::NetworkConfig,
    transport::TransportType,
};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging with modern defaults
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .with_target(false)
        .with_thread_ids(false)
        .with_file(false)
        .with_line_number(false)
        .compact()
        .finish();

    tracing::subscriber::set_global_default(subscriber)
        .expect("setting default subscriber failed");

    // Parse command line arguments
    let cli = Cli::parse();

    // Handle graceful shutdown
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::broadcast::channel(1);

    // Set up signal handling
    tokio::spawn(async move {
        let mut sigterm = tokio::signal::unix::signal(
            tokio::signal::unix::SignalKind::terminate()
        ).expect("Failed to register SIGTERM handler");

        let mut sigint = tokio::signal::unix::signal(
            tokio::signal::unix::SignalKind::interrupt()
        ).expect("Failed to register SIGINT handler");

        tokio::select! {
            _ = sigterm.recv() => {
                info!("Received SIGTERM");
            }
            _ = sigint.recv() => {
                info!("Received SIGINT");
            }
        }

        let _ = shutdown_tx.send(());
    });

    // Execute command
    match cli.command {
        ninep_server::cli::Commands::Serve(serve_cmd) => {
            info!("Starting 9P.e server with modern architecture");

            // Build server with dependency injection
            let server = Server::builder()
                .network_config(NetworkConfig {
                    bind_address: serve_cmd.bind_address(),
                    port: serve_cmd.port,
                    ipv6_dual_stack: true, // Modern default
                    prefer_ipv6: true,
                })
                .transport(if serve_cmd.quic {
                    TransportType::Quic {
                        server_name: serve_cmd.server_name.clone()
                    }
                } else {
                    TransportType::Tcp
                })
                .root_directory(serve_cmd.root.unwrap_or_else(|| PathBuf::from(".")))
                .max_message_size(serve_cmd.max_message_size)
                .worker_threads(serve_cmd.worker_threads)
                .mesh_enabled(serve_cmd.mesh_enabled)
                .mesh_port(serve_cmd.mesh_port)
                .metrics_enabled(serve_cmd.metrics_enabled)
                .metrics_port(serve_cmd.metrics_port)
                .build()
                .await?;

            info!("Server built successfully at {}", server.address());

            // Run server with graceful shutdown
            let server_handle = tokio::spawn(async move {
                if let Err(e) = server.run().await {
                    error!("Server error: {}", e);
                }
            });

            // Wait for shutdown signal
            let _ = shutdown_rx.recv().await;
            info!("Shutdown signal received, stopping server...");

            // Cancel server task
            server_handle.abort();

            // Wait for server to finish
            let _ = server_handle.await;

            info!("Server stopped gracefully");
        }

        ninep_server::cli::Commands::Client(client_cmd) => {
            info!("Client commands not yet implemented in refactored version");
            // Client implementation would go here
            match client_cmd.action {
                ninep_server::cli::commands::client::ClientAction::Connect(connect_cmd) => {
                    info!("Connect to {}", connect_cmd.address);
                    // Connection logic here
                }
                ninep_server::cli::commands::client::ClientAction::Mount(mount_cmd) => {
                    info!("Mount {} at {}", mount_cmd.remote_path, mount_cmd.local_path);
                    // Mount logic here
                }
                ninep_server::cli::commands::client::ClientAction::List(list_cmd) => {
                    info!("List directory {}", list_cmd.path);
                    // List logic here
                }
            }
        }

        ninep_server::cli::Commands::Users(users_cmd) => {
            info!("User management commands not yet implemented in refactored version");
            // User management implementation would go here
            match users_cmd.action {
                ninep_server::cli::commands::users::UserAction::Create(create_cmd) => {
                    info!("Create user {}", create_cmd.username);
                }
                ninep_server::cli::commands::users::UserAction::Delete(delete_cmd) => {
                    info!("Delete user {}", delete_cmd.username);
                }
                ninep_server::cli::commands::users::UserAction::List => {
                    info!("List users");
                }
                ninep_server::cli::commands::users::UserAction::SetPassword(passwd_cmd) => {
                    info!("Set password for user {}", passwd_cmd.username);
                }
            }
        }

        ninep_server::cli::Commands::AutoMount(automount_cmd) => {
            info!("Auto-mount commands not yet implemented in refactored version");
            // Auto-mount implementation would go here
            match automount_cmd.action {
                ninep_server::cli::commands::automount::AutoMountAction::Start(start_cmd) => {
                    info!("Start auto-mount at {}", start_cmd.mount_point);
                }
                ninep_server::cli::commands::automount::AutoMountAction::Stop(stop_cmd) => {
                    info!("Stop auto-mount at {}", stop_cmd.mount_point);
                }
                ninep_server::cli::commands::automount::AutoMountAction::List => {
                    info!("List auto-mounts");
                }
            }
        }
    }

    Ok(())
}