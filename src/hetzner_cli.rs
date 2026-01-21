use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod hetzner;

#[derive(Parser)]
#[command(name = "hetzner-cli")]
#[command(about = "Hetzner Cloud management for ffmpeg-gpc workers")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Provision a new worker VM on Hetzner
    ProvisionWorker {
        /// Hetzner API token (or set HETZNER_TOKEN env var)
        #[arg(short, long, env = "HETZNER_TOKEN")]
        token: String,
        /// Server URL (serves queue API, worker binary, and background image)
        #[arg(long)]
        server_url: String,
        /// Server name (optional, auto-generated if not provided)
        #[arg(long)]
        name: Option<String>,
    },
    /// List all servers in your Hetzner project
    ListServers {
        /// Hetzner API token (or set HETZNER_TOKEN env var)
        #[arg(short, long, env = "HETZNER_TOKEN")]
        token: String,
    },
    /// Delete a server by ID
    DeleteServer {
        /// Hetzner API token (or set HETZNER_TOKEN env var)
        #[arg(short, long, env = "HETZNER_TOKEN")]
        token: String,
        /// Server ID to delete
        #[arg(long)]
        id: u64,
    },
    /// List available server types
    ListServerTypes {
        /// Hetzner API token (or set HETZNER_TOKEN env var)
        #[arg(short, long, env = "HETZNER_TOKEN")]
        token: String,
    },
    /// List available datacenters
    ListDatacenters {
        /// Hetzner API token (or set HETZNER_TOKEN env var)
        #[arg(short, long, env = "HETZNER_TOKEN")]
        token: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::ProvisionWorker {
            token,
            server_url,
            name,
        } => {
            let base = server_url.trim_end_matches('/');
            let queue_url = format!("{}/api", base);
            let binary_url = format!("{}/assets/worker", base);
            let bg_image_url = format!("{}/assets/gpc-bg.png", base);

            let result = hetzner::provision_worker(
                &token,
                &queue_url,
                &binary_url,
                &bg_image_url,
                name,
            )
            .await?;
            println!("Worker provisioned at IP: {}", result.ip);
            if let Some(password) = result.root_password {
                println!("Root password: {}", password);
            }
        }
        Commands::ListServers { token } => {
            let client = hetzner::HetznerClient::new(token);
            let servers = client.list_servers().await?;
            if servers.is_empty() {
                println!("No servers found");
            } else {
                println!("{:<12} {:<30} {:<15} {}", "ID", "NAME", "STATUS", "IP");
                for server in servers {
                    println!(
                        "{:<12} {:<30} {:<15} {}",
                        server.id, server.name, server.status, server.public_net.ipv4.ip
                    );
                }
            }
        }
        Commands::DeleteServer { token, id } => {
            let client = hetzner::HetznerClient::new(token);
            client.delete_server(id).await?;
            println!("Server {} deleted", id);
        }
        Commands::ListServerTypes { token } => {
            let client = hetzner::HetznerClient::new(token);
            let types = client.list_server_types().await?;
            println!("{:<20} {:<10} {:<10} {:<10} {}", "NAME", "CORES", "MEMORY", "DISK", "LOCATIONS");
            for st in types {
                println!(
                    "{:<20} {:<10} {:<10} {:<10} {}",
                    st.name, st.cores, st.memory, st.disk, st.locations.join(", ")
                );
            }
        }
        Commands::ListDatacenters { token } => {
            let client = hetzner::HetznerClient::new(token);
            let dcs = client.list_datacenters().await?;
            println!("{:<20} {:<20} {}", "NAME", "LOCATION", "SERVER_TYPES");
            for dc in dcs {
                let types_str = if dc.server_types.len() > 5 {
                    format!("{} types", dc.server_types.len())
                } else {
                    dc.server_types.join(", ")
                };
                println!("{:<20} {:<20} {}", dc.name, dc.location, types_str);
            }
        }
    }

    Ok(())
}
