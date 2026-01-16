use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use tokio::time::{interval, Duration};
use tracing::{info, warn, error};
use urlencoding::encode;

pub use crate::webdav::WebDavConfig;
use crate::webdav::WebDavClient;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JobStatus {
    Pending,
    Processing,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Quadrant {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Quadrant {
    pub fn as_str(&self) -> &'static str {
        match self {
            Quadrant::TopLeft => "top-left",
            Quadrant::TopRight => "top-right",
            Quadrant::BottomLeft => "bottom-left",
            Quadrant::BottomRight => "bottom-right",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "top-left" => Some(Quadrant::TopLeft),
            "top-right" => Some(Quadrant::TopRight),
            "bottom-left" => Some(Quadrant::BottomLeft),
            "bottom-right" => Some(Quadrant::BottomRight),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoQuadrantSelection {
    pub presentation: Quadrant,
    pub slides: Quadrant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub video_path: String,
    pub output_path: String,
    pub selection: VideoQuadrantSelection,
    pub status: JobStatus,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub error: Option<String>,
    pub worker_id: Option<String>,
    pub webdav_config: WebDavConfig,
}

pub struct JobQueue {
    data_dir: String,
}

impl JobQueue {
    pub fn new(data_dir: &str) -> Self {
        Self {
            data_dir: data_dir.to_string(),
        }
    }

    fn jobs_file(&self) -> String {
        format!("{}/jobs.json", self.data_dir)
    }

    pub fn load_jobs(&self) -> Result<Vec<Job>> {
        let jobs_file_path = self.jobs_file();
        let path = std::path::Path::new(&jobs_file_path);

        if !path.exists() {
            return Ok(Vec::new());
        }

        let content = fs::read_to_string(path)?;
        let jobs: Vec<Job> = serde_json::from_str(&content)?;

        Ok(jobs)
    }

    pub fn save_jobs(&self, jobs: &[Job]) -> Result<()> {
        fs::create_dir_all(&self.data_dir)?;

        let content = serde_json::to_string_pretty(jobs)?;
        fs::write(self.jobs_file(), content)?;

        Ok(())
    }

    pub fn create_job(
        &self,
        video_path: String,
        output_path: String,
        selection: VideoQuadrantSelection,
        webdav_config: WebDavConfig,
    ) -> Result<Job> {
        let mut jobs = self.load_jobs()?;

        let job = Job {
            id: uuid::Uuid::new_v4().to_string(),
            video_path,
            output_path,
            selection,
            status: JobStatus::Pending,
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
            error: None,
            worker_id: None,
            webdav_config,
        };

        jobs.push(job.clone());
        self.save_jobs(&jobs)?;

        Ok(job)
    }

    pub fn get_pending_jobs(&self) -> Result<Vec<Job>> {
        let jobs = self.load_jobs()?;
        Ok(jobs
            .into_iter()
            .filter(|j| matches!(j.status, JobStatus::Pending))
            .collect())
    }

    pub fn update_job_status(&self, job_id: &str, status: JobStatus) -> Result<Job> {
        let mut jobs = self.load_jobs()?;
        let job = jobs
            .iter_mut()
            .find(|j| j.id == job_id)
            .ok_or_else(|| anyhow::anyhow!("Job not found: {}", job_id))?;

        job.status = status.clone();

        match status {
            JobStatus::Processing => {
                job.started_at = Some(Utc::now());
            }
            JobStatus::Completed | JobStatus::Failed => {
                job.completed_at = Some(Utc::now());
            }
            _ => {}
        }

        let job = job.clone();
        self.save_jobs(&jobs)?;

        Ok(job)
    }

    pub fn get_job(&self, job_id: &str) -> Result<Job> {
        let jobs = self.load_jobs()?;
        jobs
            .into_iter()
            .find(|j| j.id == job_id)
            .ok_or_else(|| anyhow::anyhow!("Job not found: {}", job_id))
    }

    pub fn list_jobs(&self) -> Result<Vec<Job>> {
        self.load_jobs()
    }
}

pub async fn run_worker(queue_url: String) -> Result<()> {
    info!("Starting worker, polling queue at: {}", queue_url);

    let mut tick = interval(Duration::from_secs(10));

    loop {
        tick.tick().await;

        // Poll for jobs
        match poll_queue(&queue_url).await {
            Ok(Some(job)) => {
                info!("Received job: {}", job.id);
                if let Err(e) = process_job(job).await {
                    error!("Job processing failed: {}", e);
                }
            }
            Ok(None) => {
                info!("No jobs available");
            }
            Err(e) => {
                warn!("Failed to poll queue: {}", e);
            }
        }
    }
}

async fn poll_queue(queue_url: &str) -> Result<Option<Job>> {
    let client = reqwest::Client::new();
    let response = client
        .get(format!("{}/jobs/pending", queue_url))
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to poll queue: {}", e))?;

    let status = response.status();
    if status.as_u16() == 204 {
        return Ok(None);
    }

    let job: Job = response
        .json()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to parse job: {}", e))?;
    Ok(Some(job))
}

async fn process_job(job: Job) -> Result<()> {
    info!("Processing job: {}", job.id);

    let worker_id = format!("worker-{}", uuid::Uuid::new_v4().simple());
    let temp_dir = format!("./worker-temp-{}", worker_id);
    fs::create_dir_all(&temp_dir)?;

    // Build input URL with auth for direct FFmpeg streaming
    let video_url = build_webdav_download_url(&job.webdav_config, &job.video_path);
    let _input_path = format!("{}/input.mp4", temp_dir);

    // Background image path (downloaded by cloud-init to /root)
    let bg_image_path = "/root/gpc-bg.png";

    let output_path = format!("{}/output.mp4", temp_dir);

    // Build FFmpeg filter complex based on quadrant selection
    let filter_complex = build_filter_complex(&job.selection)?;

    info!("Running FFmpeg with filter: {}", filter_complex);

    // Run FFmpeg command
    let ffmpeg_result = tokio::process::Command::new("ffmpeg")
        .arg("-y")  // Overwrite output
        .arg("-i").arg(&video_url)  // Input video
        .arg("-i").arg(bg_image_path)  // Background image
        .arg("-filter_complex").arg(&filter_complex)
        .arg("-map").arg("[outv]")
        .arg("-map").arg("0:a?")
        .arg("-c:v").arg("libx264")
        .arg("-crf").arg("18")
        .arg("-preset").arg("veryfast")
        .arg("-threads").arg("0")
        .arg("-c:a").arg("copy")
        .arg(&output_path)
        .output()
        .await;

    match ffmpeg_result {
        Ok(output) => {
            if output.status.success() {
                info!("FFmpeg processing successful");

                // Upload result back to WebDAV
                let output_data = fs::read(&output_path)?;
                let dav_client = WebDavClient::new(&job.webdav_config)?;

                info!("Uploading processed video to: {}", job.output_path);
                dav_client.upload_file(&job.output_path, output_data).await?;

                info!("Job {} completed successfully", job.id);

                // Update job to completed via queue URL
                if let Some(queue_url) = &job.webdav_config.queue_url {
                    let _ = update_job_status_remote(queue_url, &job.id, JobStatus::Completed, None).await;
                }
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                error!("FFmpeg failed: {}", stderr);

                if let Some(queue_url) = &job.webdav_config.queue_url {
                    let _ = update_job_status_remote(queue_url, &job.id, JobStatus::Failed, None).await;
                }
            }
        }
        Err(e) => {
            error!("Failed to run FFmpeg: {}", e);

            if let Some(queue_url) = &job.webdav_config.queue_url {
                let _ = update_job_status_remote(queue_url, &job.id, JobStatus::Failed, None).await;
            }
        }
    }

    // Cleanup temp directory
    let _ = fs::remove_dir_all(&temp_dir);

    Ok(())
}

fn build_filter_complex(selection: &VideoQuadrantSelection) -> Result<String> {
    // Video is 3840x2160 (4K), divided into 4 quadrants of 1920x1080 each
    // We apply a 4px offset to trim borders from the presentation quadrant

    fn quadrant_crop(q: &Quadrant) -> (u32, u32, u32, u32) {
        // Returns (width, height, x, y)
        match q {
            Quadrant::TopLeft => (1912, 1072, 4, 4),
            Quadrant::TopRight => (1912, 1072, 1924, 4),
            Quadrant::BottomLeft => (1912, 1072, 4, 1084),
            Quadrant::BottomRight => (1912, 1072, 1924, 1084),
        }
    }

    let (pw, ph, px, py) = quadrant_crop(&selection.presentation);
    let (sw, sh, sx, sy) = quadrant_crop(&selection.slides);

    let pres_crop = format!("{}:{}:{}:{}", pw, ph, px, py);
    let speaker_crop = format!("{}:{}:{}:{}", sw, sh, sx, sy);

    info!("Presentation crop: {}, Speaker crop: {}", pres_crop, speaker_crop);

    Ok(format!(
        "[1:v]scale=2560:1440[bg]; \
         [0:v]crop={}[pres_cropped]; \
         [pres_cropped]scale=1920:1080[pres]; \
         [0:v]crop={}[speaker_raw]; \
         [speaker_raw]scale=-1:320[speaker]; \
         [pres]scale=1920:1080[pres_s]; \
         [bg][pres_s]overlay=(W-w)/2:(H-h)/2[base]; \
         [base][speaker]overlay=x=W-w-40:y=H-h-40[outv]",
        pres_crop, speaker_crop
    ))
}

fn build_webdav_download_url(config: &WebDavConfig, path: &str) -> String {
    // Extract server base URL (protocol + hostname) and build direct download URL
    let server_base = if let Some(pos) = config.url.find("/remote.php") {
        &config.url[..pos]
    } else {
        &config.url
    };

    // For NextCloud, the direct download URL might be different from WebDAV URL
    // Use the path directly with credentials embedded
    format!("{}{}",
        server_base,
        path
    )
    .replacen("://", &format!("://{}:{}@", encode(&config.username), encode(&config.password)), 1)
}

async fn update_job_status_remote(
    queue_url: &str,
    job_id: &str,
    status: JobStatus,
    worker_id: Option<&str>,
) -> Result<()> {
    let client = reqwest::Client::new();

    #[derive(Serialize)]
    struct StatusUpdate {
        status: String,
        worker_id: Option<String>,
    }

    let update = StatusUpdate {
        status: match status {
            JobStatus::Pending => "pending".to_string(),
            JobStatus::Processing => "processing".to_string(),
            JobStatus::Completed => "completed".to_string(),
            JobStatus::Failed => "failed".to_string(),
        },
        worker_id: worker_id.map(|s| s.to_string()),
    };

    client
        .patch(format!("{}/jobs/{}", queue_url, job_id))
        .json(&update)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to update job status: {}", e))?;

    Ok(())
}
