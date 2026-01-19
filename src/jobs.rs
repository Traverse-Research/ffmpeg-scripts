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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct JobProgress {
    /// Current frame being processed
    pub frame: Option<u64>,
    /// Total frames (if known)
    pub total_frames: Option<u64>,
    /// Current time position in the video (e.g., "00:01:23.45")
    pub time: Option<String>,
    /// Total duration (e.g., "00:10:00.00")
    pub duration: Option<String>,
    /// Processing speed (e.g., "1.5x")
    pub speed: Option<String>,
    /// Percentage complete (0-100)
    pub percent: Option<f32>,
    /// Current stage of processing
    pub stage: Option<String>,
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
    #[serde(default)]
    pub progress: Option<JobProgress>,
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
            progress: None,
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

    /// Atomically claim a pending job for a worker.
    /// Returns the job if one was claimed, None if no pending jobs exist.
    pub fn claim_job(&self, worker_id: &str) -> Result<Option<Job>> {
        let mut jobs = self.load_jobs()?;

        // Find first pending job
        let job = jobs
            .iter_mut()
            .find(|j| matches!(j.status, JobStatus::Pending));

        match job {
            Some(job) => {
                // Atomically mark as processing and assign worker
                job.status = JobStatus::Processing;
                job.started_at = Some(Utc::now());
                job.worker_id = Some(worker_id.to_string());

                let claimed_job = job.clone();
                self.save_jobs(&jobs)?;

                Ok(Some(claimed_job))
            }
            None => Ok(None),
        }
    }

    /// Update progress for a job
    pub fn update_job_progress(&self, job_id: &str, progress: JobProgress) -> Result<Job> {
        let mut jobs = self.load_jobs()?;
        let job = jobs
            .iter_mut()
            .find(|j| j.id == job_id)
            .ok_or_else(|| anyhow::anyhow!("Job not found: {}", job_id))?;

        job.progress = Some(progress);
        let job = job.clone();
        self.save_jobs(&jobs)?;

        Ok(job)
    }
}

pub async fn run_worker(queue_url: String) -> Result<()> {
    // Generate a unique worker ID for this instance
    let worker_id = format!("worker-{}", uuid::Uuid::new_v4().simple());
    info!("Starting worker {} polling queue at: {}", worker_id, queue_url);

    let mut tick = interval(Duration::from_secs(10));

    loop {
        tick.tick().await;

        // Try to claim a job atomically
        match claim_job(&queue_url, &worker_id).await {
            Ok(Some(job)) => {
                info!("Worker {} claimed job: {}", worker_id, job.id);
                if let Err(e) = process_job(job).await {
                    error!("Job processing failed: {}", e);
                }
            }
            Ok(None) => {
                info!("No jobs available");
            }
            Err(e) => {
                warn!("Failed to claim job: {}", e);
            }
        }
    }
}

async fn claim_job(queue_url: &str, worker_id: &str) -> Result<Option<Job>> {
    let url = format!("{}/jobs/claim", queue_url);
    info!("Claiming job at: {}", url);

    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .json(&serde_json::json!({ "worker_id": worker_id }))
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to claim job: {}", e))?;

    let status = response.status();
    info!("Claim response status: {}", status);

    if status.as_u16() == 204 {
        return Ok(None);
    }

    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("Claim failed with status {}: {}", status, body));
    }

    let body = response.text().await.map_err(|e| anyhow::anyhow!("Failed to read response: {}", e))?;
    info!("Claim response body: {}", body);

    let job: Job = serde_json::from_str(&body)
        .map_err(|e| anyhow::anyhow!("Failed to parse job: {} - body was: {}", e, body))?;
    info!("Claimed job: {} with status {:?}", job.id, job.status);
    Ok(Some(job))
}

async fn process_job(job: Job) -> Result<()> {
    info!("=== PROCESSING JOB START ===");
    info!("Job ID: {}", job.id);
    info!("Video path: {}", job.video_path);
    info!("Output path: {}", job.output_path);
    info!("Selection: {:?}", job.selection);
    info!("WebDAV URL: {}", job.webdav_config.url);
    info!("Queue URL: {:?}", job.webdav_config.queue_url);

    let worker_id = format!("worker-{}", uuid::Uuid::new_v4().simple());
    let temp_dir = format!("/tmp/worker-{}", worker_id);
    info!("Creating temp dir: {}", temp_dir);
    fs::create_dir_all(&temp_dir)?;

    // Build input URL with auth for direct FFmpeg streaming
    let video_url = build_webdav_download_url(&job.webdav_config, &job.video_path);
    info!("Video URL for FFmpeg: {}", video_url);

    // Background image path (downloaded by cloud-init to /root)
    let bg_image_path = "/root/gpc-bg.png";
    info!("Background image path: {}", bg_image_path);

    // Check if background image exists
    if std::path::Path::new(bg_image_path).exists() {
        info!("Background image exists at {}", bg_image_path);
    } else {
        error!("Background image NOT FOUND at {}", bg_image_path);
        // Try to list /root to see what's there
        if let Ok(entries) = std::fs::read_dir("/root") {
            info!("Contents of /root:");
            for entry in entries {
                if let Ok(e) = entry {
                    info!("  - {:?}", e.path());
                }
            }
        }
    }

    // Build FFmpeg filter complex based on quadrant selection
    let filter_complex = build_filter_complex(&job.selection)?;
    info!("FFmpeg filter: {}", filter_complex);

    // Local output path for FFmpeg
    let local_output_path = format!("{}/output.mp4", temp_dir);
    info!("Local output path: {}", local_output_path);

    // Report initial progress
    if let Some(queue_url) = &job.webdav_config.queue_url {
        let _ = update_job_progress_remote(queue_url, &job.id, JobProgress {
            stage: Some("Starting FFmpeg".to_string()),
            ..Default::default()
        }).await;
    }

    info!("Starting FFmpeg (output to local file)...");

    // Run FFmpeg command with progress parsing
    // Use -progress pipe:1 to get machine-readable progress on stdout
    let mut child = tokio::process::Command::new("ffmpeg")
        .arg("-y")  // Overwrite output
        .arg("-progress").arg("pipe:1")  // Output progress to stdout
        .arg("-i").arg(&video_url)  // Input video (streaming from WebDAV)
        .arg("-i").arg(bg_image_path)  // Background image
        .arg("-filter_complex").arg(&filter_complex)
        .arg("-map").arg("[outv]")
        .arg("-map").arg("0:a?")
        .arg("-c:v").arg("libx264")
        .arg("-crf").arg("18")
        .arg("-preset").arg("veryfast")
        .arg("-threads").arg("0")
        .arg("-c:a").arg("copy")
        .arg(&local_output_path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    // Parse progress from stdout
    let stdout = child.stdout.take();
    let queue_url_clone = job.webdav_config.queue_url.clone();
    let job_id_clone = job.id.clone();

    // Spawn a task to read and parse progress
    let progress_handle = tokio::spawn(async move {
        if let Some(stdout) = stdout {
            use tokio::io::{AsyncBufReadExt, BufReader};
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();

            let mut current_frame: Option<u64> = None;
            let mut current_time: Option<String> = None;
            let mut current_speed: Option<String> = None;
            let mut total_duration: Option<String> = None;
            let mut last_report = std::time::Instant::now();
            let mut progress_count = 0u32;

            info!("Starting to read FFmpeg progress from stdout...");

            while let Ok(Some(line)) = lines.next_line().await {
                // Parse FFmpeg progress output format:
                // frame=123
                // fps=30.0
                // out_time=00:00:05.123456
                // speed=1.5x
                // progress=continue/end

                if let Some(value) = line.strip_prefix("frame=") {
                    current_frame = value.trim().parse().ok();
                } else if let Some(value) = line.strip_prefix("out_time=") {
                    // Format: 00:00:05.123456 - trim to 00:00:05
                    let time = value.trim();
                    if let Some(dot_pos) = time.rfind('.') {
                        current_time = Some(time[..dot_pos].to_string());
                    } else {
                        current_time = Some(time.to_string());
                    }
                } else if let Some(value) = line.strip_prefix("speed=") {
                    current_speed = Some(value.trim().to_string());
                } else if let Some(value) = line.strip_prefix("duration=") {
                    total_duration = Some(value.trim().to_string());
                } else if line.starts_with("progress=") {
                    progress_count += 1;
                    // End of a progress block - report to server (throttled)
                    if last_report.elapsed() >= std::time::Duration::from_secs(2) {
                        if let Some(queue_url) = &queue_url_clone {
                            info!("Sending progress update #{}: frame={:?}, time={:?}, speed={:?}",
                                  progress_count, current_frame, current_time, current_speed);
                            let progress = JobProgress {
                                frame: current_frame,
                                total_frames: None,
                                time: current_time.clone(),
                                duration: total_duration.clone(),
                                speed: current_speed.clone(),
                                percent: None, // Could calculate from time/duration
                                stage: Some("Encoding".to_string()),
                            };
                            match update_job_progress_remote(queue_url, &job_id_clone, progress).await {
                                Ok(_) => info!("Progress update sent successfully"),
                                Err(e) => error!("Failed to send progress update: {}", e),
                            }
                        }
                        last_report = std::time::Instant::now();
                    }
                }
            }
            info!("Finished reading FFmpeg progress. Total progress blocks: {}", progress_count);
        } else {
            warn!("No stdout available from FFmpeg process");
        }
    });

    // Read stderr in a separate task
    let stderr_handle = {
        let stderr = child.stderr.take();
        tokio::spawn(async move {
            if let Some(stderr) = stderr {
                use tokio::io::AsyncReadExt;
                let mut buf = String::new();
                let mut reader = tokio::io::BufReader::new(stderr);
                let _ = reader.read_to_string(&mut buf).await;
                buf
            } else {
                String::new()
            }
        })
    };

    // Wait for FFmpeg to complete
    let status = child.wait().await?;

    // Wait for progress parsing and stderr reading to finish
    let _ = progress_handle.await;
    let stderr = stderr_handle.await.unwrap_or_default();

    info!("FFmpeg exit status: {}", status);
    if !stderr.is_empty() {
        info!("FFmpeg stderr: {}", stderr);
    }

    if status.success() {
        info!("FFmpeg processing successful!");

        // Report upload stage
        if let Some(queue_url) = &job.webdav_config.queue_url {
            let _ = update_job_progress_remote(queue_url, &job.id, JobProgress {
                stage: Some("Uploading".to_string()),
                ..Default::default()
            }).await;
        }

        // Check output file size
        match fs::metadata(&local_output_path) {
            Ok(meta) => info!("Output file size: {} bytes", meta.len()),
            Err(e) => error!("Failed to stat output file: {}", e),
        }

        // Now upload to WebDAV
        info!("Reading output file for upload...");
        let output_data = fs::read(&local_output_path)?;
        info!("Read {} bytes, uploading to WebDAV...", output_data.len());

        let dav_client = WebDavClient::new(&job.webdav_config)?;

        // Create the output folder on WebDAV if needed
        // job.output_path is like "processed/filename.mp4"
        if let Some(folder_end) = job.output_path.rfind('/') {
            let folder = &job.output_path[..folder_end];
            if !folder.is_empty() {
                info!("Ensuring folder exists: {}", folder);
                if let Err(e) = dav_client.ensure_folder_exists(folder).await {
                    warn!("Could not create folder {}: {} (may already exist)", folder, e);
                }
            }
        }

        info!("Uploading to: {}", job.output_path);
        match dav_client.upload_file(&job.output_path, output_data).await {
            Ok(_) => {
                info!("Upload successful!");
                info!("Job {} completed successfully", job.id);

                // Update job to completed via queue URL
                if let Some(queue_url) = &job.webdav_config.queue_url {
                    info!("Updating job status to completed at: {}", queue_url);
                    match update_job_status_remote(queue_url, &job.id, JobStatus::Completed, None).await {
                        Ok(_) => info!("Status update successful"),
                        Err(e) => error!("Status update failed: {}", e),
                    }
                }
            }
            Err(e) => {
                error!("Upload FAILED: {}", e);
                if let Some(queue_url) = &job.webdav_config.queue_url {
                    let _ = update_job_status_remote(queue_url, &job.id, JobStatus::Failed, None).await;
                }
            }
        }
    } else {
        error!("FFmpeg FAILED with exit code: {}", status);

        if let Some(queue_url) = &job.webdav_config.queue_url {
            let _ = update_job_status_remote(queue_url, &job.id, JobStatus::Failed, None).await;
        }
    }

    // Cleanup temp directory
    info!("Cleaning up temp dir: {}", temp_dir);
    let _ = fs::remove_dir_all(&temp_dir);

    info!("=== PROCESSING JOB END ===");
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

async fn update_job_progress_remote(
    queue_url: &str,
    job_id: &str,
    progress: JobProgress,
) -> Result<()> {
    let client = reqwest::Client::new();

    client
        .patch(format!("{}/jobs/{}/progress", queue_url, job_id))
        .json(&progress)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to update job progress: {}", e))?;

    Ok(())
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
