mod api;
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
    }

    Ok(())
}
