mod api;
mod hetzner;
mod jobs;
mod processing;
mod webdav;

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser)]
#[command(name = "ffmpeg-gpc")]
#[command(about = "Video processing pipeline for conference recordings", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the web server for video tagging
    Server {
        #[arg(short, long, default_value_t = 3000)]
        port: u16,
        #[arg(long, default_value = "data")]
        data_dir: String,
    },
    /// Process a single video (for testing)
    Process {
        input: String,
        output: String,
        /// Presentation quadrant (top-left, top-right, bottom-left, bottom-right)
        #[arg(long, default_value = "top-left")]
        presentation: String,
        /// Speaker/slides quadrant (top-left, top-right, bottom-left, bottom-right)
        #[arg(long, default_value = "top-right")]
        speaker: String,
    },
    /// Worker that runs on Hetzner VM
    Worker {
        /// Job queue URL to poll
        #[arg(short, long)]
        queue_url: String,
    },
    /// List videos from WebDAV
    List {
        webdav_url: String,
        username: String,
        password: String,
        path: String,
    },
    /// Hetzner cloud operations
    Hetzner {
        #[command(subcommand)]
        hetzner_command: HetznerCommands,
    },
}

#[derive(Subcommand)]
enum HetznerCommands {
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
    /// List all Hetzner servers
    ListServers {
        /// Hetzner API token (or set HETZNER_TOKEN env var)
        #[arg(short, long, env = "HETZNER_TOKEN")]
        token: String,
    },
    /// Delete a Hetzner server
    DeleteServer {
        /// Hetzner API token (or set HETZNER_TOKEN env var)
        #[arg(short, long, env = "HETZNER_TOKEN")]
        token: String,
        /// Server ID to delete
        id: u64,
    },
    /// Generate cloud-init config for manual use
    CloudInit {
        /// Queue URL for the worker to poll
        #[arg(long)]
        queue_url: String,
        /// URL to download the worker binary from
        #[arg(long)]
        binary_url: String,
        /// URL to download the background image from
        #[arg(long)]
        bg_image_url: String,
        /// Optional SSH public key to add
        #[arg(long)]
        ssh_key: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    match cli.command {
        Commands::Server { port, data_dir } => {
            api::run_server(port, &data_dir).await?;
        }
        Commands::Process {
            input,
            output,
            presentation,
            speaker,
        } => {
            use crate::jobs::{Quadrant, VideoQuadrantSelection};
            let pres = Quadrant::from_str(&presentation).ok_or_else(|| {
                anyhow::anyhow!("Invalid presentation quadrant: {}", presentation)
            })?;
            let spk = Quadrant::from_str(&speaker).ok_or_else(|| {
                anyhow::anyhow!("Invalid speaker quadrant: {}", speaker)
            })?;
            let selection = VideoQuadrantSelection {
                presentation: pres,
                slides: spk,
            };
            processing::process_video_with_selection(&input, &output, &selection).await?;
        }
        Commands::Worker { queue_url } => {
            jobs::run_worker(queue_url).await?;
        }
        Commands::List {
            webdav_url,
            username,
            password,
            path,
        } => {
            webdav::list_videos(&webdav_url, &username, &password, &path).await?;
        }
        Commands::Hetzner { hetzner_command } => match hetzner_command {
            HetznerCommands::ProvisionWorker {
                token,
                server_url,
                name,
            } => {
                let base = server_url.trim_end_matches('/');
                let queue_url = format!("{}/api", base);
                let binary_url = format!("{}/assets/worker", base);
                let bg_image_url = format!("{}/assets/gpc-bg.png", base);

                let ip = hetzner::provision_worker(
                    &token,
                    &queue_url,
                    &binary_url,
                    &bg_image_url,
                    name,
                )
                .await?;
                println!("Worker provisioned at IP: {}", ip);
            }
            HetznerCommands::ListServers { token } => {
                let client = hetzner::HetznerClient::new(token);
                let servers = client.list_servers().await?;
                println!("Found {} servers:", servers.len());
                for server in servers {
                    println!(
                        "  {} ({}): {} - {}",
                        server.id, server.name, server.status, server.public_net.ipv4.ip
                    );
                }
            }
            HetznerCommands::DeleteServer { token, id } => {
                let client = hetzner::HetznerClient::new(token);
                client.delete_server(id).await?;
                println!("Server {} deleted", id);
            }
            HetznerCommands::CloudInit {
                queue_url,
                binary_url,
                bg_image_url,
                ssh_key,
            } => {
                let cloud_init = if let Some(key) = ssh_key {
                    hetzner::worker_cloud_init_with_ssh(
                        &queue_url,
                        &binary_url,
                        &bg_image_url,
                        &key,
                    )
                } else {
                    hetzner::HetznerClient::worker_cloud_init(
                        &queue_url,
                        &binary_url,
                        &bg_image_url,
                    )
                };
                println!("{}", cloud_init);
            }
        },
    }

    Ok(())
}
