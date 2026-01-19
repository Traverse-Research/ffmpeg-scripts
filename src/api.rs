use crate::jobs::{Job, JobProgress, JobQueue, JobStatus, Quadrant, VideoQuadrantSelection, WebDavConfig};
use crate::webdav::WebDavClient;
use anyhow::Result;
use axum::{
    extract::{Path, Query, State},
    response::{Html, IntoResponse, Json, Response},
    routing::{get, post, patch},
    Router,
};
use http::StatusCode;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::fs;
use tower_http::cors::CorsLayer;
use tracing::{error, info, debug};

// Wrapper for error responses
pub struct AppError {
    status: StatusCode,
    message: String,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (self.status, Json(ErrorResponse { error: self.message })).into_response()
    }
}

#[derive(Clone)]
pub struct AppState {
    pub queue: Arc<Mutex<JobQueue>>,
    pub preview_cache: Arc<Mutex<HashMap<String, Vec<String>>>>,
    pub data_dir: String,
    pub public_url: String,
}

#[derive(Debug, Deserialize)]
struct WebDavQuery {
    url: String,
    username: String,
    password: String,
    path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CreateJobRequest {
    video_path: String,
    output_path: String,
    presentation_quadrant: String,
    slides_quadrant: String,
    webdav_url: String,
    webdav_username: String,
    webdav_password: String,
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

pub async fn run_server(port: u16, data_dir: &str) -> Result<()> {
    // Log default WebDAV config from environment if available
    if let Ok(url) = std::env::var("WEBDAV_URL") {
        info!("Default WebDAV URL configured: {}", url);
    }

    // Get public URL for workers to callback to
    let public_url = std::env::var("PUBLIC_URL")
        .unwrap_or_else(|_| format!("http://localhost:{}", port));
    info!("Public URL: {}", public_url);

    let state = AppState {
        queue: Arc::new(Mutex::new(JobQueue::new(data_dir))),
        preview_cache: Arc::new(Mutex::new(HashMap::new())),
        data_dir: data_dir.to_string(),
        public_url,
    };

    fs::create_dir_all(format!("{}/previews", data_dir)).await?;

    let app = Router::new()
        .route("/", get(index))
        .route("/api/videos", get(list_videos))
        .route("/api/videos/preview", get(get_previews))
        .route("/api/jobs", post(create_job))
        .route("/api/jobs", get(list_jobs))
        .route("/api/jobs/{id}", get(get_job))
        .route("/api/jobs/{id}", patch(update_job))
        .route("/api/jobs/pending", get(get_pending_job))
        .route("/api/jobs/claim", post(claim_job))
        .route("/api/jobs/{id}/progress", patch(update_job_progress))
        .route("/health", get(health_check))
        // Static files for worker provisioning
        .route("/assets/worker", get(serve_worker_binary))
        .route("/assets/gpc-bg.png", get(serve_background_image))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    info!("Server listening on http://0.0.0.0:{}", port);

    axum::serve(listener, app).await?;

    Ok(())
}

async fn index() -> Html<&'static str> {
    Html(include_str!("../templates/index.html"))
}

async fn health_check() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "healthy",
        "timestamp": chrono::Utc::now().to_rfc3339()
    }))
}

async fn list_videos(Query(params): Query<WebDavQuery>) -> Response {
    let config = WebDavConfig {
        url: params.url.clone(),
        username: params.username,
        password: params.password,
        queue_url: None,
    };

    let client = match WebDavClient::new(&config) {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to create WebDAV client: {}", e);
            return AppError {
                status: StatusCode::BAD_REQUEST,
                message: format!("Invalid WebDAV configuration: {}", e),
            }.into_response();
        }
    };

    let path = params.path.as_deref().unwrap_or("/");
    let videos = match client.list_videos(path).await {
        Ok(v) => v,
        Err(e) => {
            error!("Failed to list videos: {}", e);
            return AppError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: format!("Failed to list videos: {}", e),
            }.into_response();
        }
    };

    Json(videos).into_response()
}

async fn get_previews(
    Query(params): Query<WebDavQuery>,
) -> Response {
    debug!("get_previews called with path: {:?}", params.path);

    let config = WebDavConfig {
        url: params.url.trim_end_matches('/').to_string(),
        username: params.username.clone(),
        password: params.password,
        queue_url: None,
    };

    let path = params.path.unwrap_or_else(|| "/".to_string());

    // The path from WebDAV already contains the full URL path relative to the server root
    // We need to construct the full URL by replacing the WebDAV base path with the server
    // For NextCloud: WebDAV URL is like https://server/remote.php/webdav/
    // and the path is like /remote.php/dav/files/user/path/to/video.mp4
    // So we just need to prepend the server base (https://server) to the path

    // Extract the server base URL (protocol + hostname + port if any)
    let server_base = if let Some(pos) = config.url.find("/remote.php") {
        &config.url[..pos]
    } else if let Some(pos) = config.url.find("/remote.php") {
        &config.url[..pos]
    } else {
        // Fallback: just use the config URL as-is
        &config.url
    };

    let video_url = format!("{}{}", server_base, path);

    info!("Extracting previews from URL: {}", video_url);

    let preview_dir = "data/previews";

    // Create preview directory
    if let Err(e) = fs::create_dir_all(preview_dir).await {
        return AppError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!("Failed to create preview directory: {}", e),
        }.into_response();
    }

    // Extract frames directly from the HTTP URL using FFmpeg's seeking
    // This only downloads the necessary parts of the video, not the whole file
    let frames = match crate::processing::extract_preview_frames_from_url_with_auth(
        &video_url,
        preview_dir,
        Some(&config.username),
        Some(&config.password),
    ) {
        Ok(f) => f,
        Err(e) => {
            return AppError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: format!("Failed to extract frames from URL: {}", e),
            }.into_response();
        }
    };

    // Encode frames as base64
    let mut result = HashMap::new();
    for (i, frame_path) in frames.iter().enumerate() {
        let name = match i {
            0 => "first",
            1 => "middle",
            2 => "last",
            _ => "unknown",
        };
        if let Ok(data) = fs::read(frame_path).await {
            use base64::prelude::*;
            result.insert(
                name.to_string(),
                format!("data:image/jpeg;base64,{}", BASE64_STANDARD.encode(&data)),
            );
        }
    }

    Json(result).into_response()
}

async fn create_job(
    State(state): State<AppState>,
    Json(req): Json<CreateJobRequest>,
) -> Response {
    let presentation = match Quadrant::from_str(&req.presentation_quadrant) {
        Some(q) => q,
        None => {
            return AppError {
                status: StatusCode::BAD_REQUEST,
                message: format!("Invalid presentation quadrant: {}", req.presentation_quadrant),
            }.into_response();
        }
    };

    let slides = match Quadrant::from_str(&req.slides_quadrant) {
        Some(q) => q,
        None => {
            return AppError {
                status: StatusCode::BAD_REQUEST,
                message: format!("Invalid slides quadrant: {}", req.slides_quadrant),
            }.into_response();
        }
    };

    let selection = VideoQuadrantSelection { presentation, slides };

    let webdav_config = WebDavConfig {
        url: req.webdav_url,
        username: req.webdav_username,
        password: req.webdav_password,
        queue_url: Some(state.public_url.clone()),
    };

    let queue = state.queue.lock().unwrap();
    match queue.create_job(
        req.video_path,
        req.output_path,
        selection,
        webdav_config,
    ) {
        Ok(job) => {
            info!("Created job: {}", job.id);
            Json(job).into_response()
        }
        Err(e) => AppError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!("Failed to create job: {}", e),
        }.into_response(),
    }
}

async fn list_jobs(State(state): State<AppState>) -> Json<Vec<Job>> {
    let queue = state.queue.lock().unwrap();
    match queue.list_jobs() {
        Ok(jobs) => Json(jobs),
        Err(_) => Json(Vec::new()),
    }
}

async fn get_job(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let queue = state.queue.lock().unwrap();
    match queue.get_job(&id) {
        Ok(job) => Json(job).into_response(),
        Err(e) => AppError {
            status: StatusCode::NOT_FOUND,
            message: format!("Job not found: {}", e),
        }.into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct UpdateJobRequest {
    status: String,
    worker_id: Option<String>,
}

async fn update_job(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateJobRequest>,
) -> Response {
    let status = match req.status.as_str() {
        "pending" => JobStatus::Pending,
        "processing" => JobStatus::Processing,
        "completed" => JobStatus::Completed,
        "failed" => JobStatus::Failed,
        _ => {
            return AppError {
                status: StatusCode::BAD_REQUEST,
                message: format!("Invalid status: {}", req.status),
            }.into_response();
        }
    };

    let queue = state.queue.lock().unwrap();
    match queue.update_job_status(&id, status.clone()) {
        Ok(job) => {
            // Update worker_id if provided - need to reload jobs, update, and save
            if let Some(worker) = &req.worker_id {
                let mut all_jobs = queue.load_jobs().unwrap_or_default();
                if let Some(j) = all_jobs.iter_mut().find(|j| j.id == id) {
                    j.worker_id = Some(worker.clone());
                    let _ = queue.save_jobs(&all_jobs);
                }
            }
            info!("Updated job {} to {:?}", id, status);
            Json(job).into_response()
        }
        Err(e) => AppError {
            status: StatusCode::NOT_FOUND,
            message: format!("Job not found: {}", e),
        }.into_response(),
    }
}

async fn get_pending_job(State(state): State<AppState>) -> Response {
    let queue = state.queue.lock().unwrap();
    match queue.get_pending_jobs() {
        Ok(jobs) if !jobs.is_empty() => {
            // Get the first pending job and mark it as processing
            let job = &jobs[0];
            let job_id = job.id.clone();
            drop(queue); // Release lock before updating

            let queue = state.queue.lock().unwrap();
            match queue.update_job_status(&job_id, JobStatus::Processing) {
                Ok(updated_job) => Json(updated_job).into_response(),
                Err(e) => AppError {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    message: format!("Failed to update job: {}", e),
                }.into_response(),
            }
        }
        Ok(_) => {
            // No pending jobs
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => AppError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!("Failed to get pending jobs: {}", e),
        }.into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct ClaimJobRequest {
    worker_id: String,
}

/// Atomically claim a pending job for a worker.
/// This prevents multiple workers from claiming the same job.
async fn claim_job(
    State(state): State<AppState>,
    Json(req): Json<ClaimJobRequest>,
) -> Response {
    let queue = state.queue.lock().unwrap();
    match queue.claim_job(&req.worker_id) {
        Ok(Some(job)) => {
            info!("Worker {} claimed job {}", req.worker_id, job.id);
            Json(job).into_response()
        }
        Ok(None) => {
            // No pending jobs available
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => {
            error!("Failed to claim job: {}", e);
            AppError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: format!("Failed to claim job: {}", e),
            }.into_response()
        }
    }
}

#[derive(Debug, Deserialize)]
struct UpdateProgressRequest {
    #[serde(default)]
    frame: Option<u64>,
    #[serde(default)]
    total_frames: Option<u64>,
    #[serde(default)]
    time: Option<String>,
    #[serde(default)]
    duration: Option<String>,
    #[serde(default)]
    speed: Option<String>,
    #[serde(default)]
    percent: Option<f32>,
    #[serde(default)]
    stage: Option<String>,
}

/// Update progress for a job
async fn update_job_progress(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateProgressRequest>,
) -> Response {
    let progress = JobProgress {
        frame: req.frame,
        total_frames: req.total_frames,
        time: req.time,
        duration: req.duration,
        speed: req.speed,
        percent: req.percent,
        stage: req.stage,
    };

    let queue = state.queue.lock().unwrap();
    match queue.update_job_progress(&id, progress) {
        Ok(job) => {
            Json(job).into_response()
        }
        Err(e) => {
            error!("Failed to update job progress: {}", e);
            AppError {
                status: StatusCode::NOT_FOUND,
                message: format!("Job not found: {}", e),
            }.into_response()
        }
    }
}

async fn serve_worker_binary() -> Response {
    // Serve the Linux worker binary from ./assets/worker-linux
    let path = "./assets/worker-linux";
    match fs::read(path).await {
        Ok(data) => {
            (
                StatusCode::OK,
                [
                    ("content-type", "application/octet-stream"),
                    ("content-disposition", "attachment; filename=\"worker\""),
                ],
                data,
            )
                .into_response()
        }
        Err(e) => {
            error!("Failed to read worker binary from {}: {}", path, e);
            AppError {
                status: StatusCode::NOT_FOUND,
                message: format!("Worker binary not found. Place Linux binary at {}", path),
            }
            .into_response()
        }
    }
}

async fn serve_background_image() -> Response {
    // Serve the background image from ./gpc-bg.png
    let path = "./gpc-bg.png";
    match fs::read(path).await {
        Ok(data) => {
            (
                StatusCode::OK,
                [("content-type", "image/png")],
                data,
            )
                .into_response()
        }
        Err(e) => {
            error!("Failed to read background image from {}: {}", path, e);
            AppError {
                status: StatusCode::NOT_FOUND,
                message: format!("Background image not found. Place gpc-bg.png at {}", path),
            }
            .into_response()
        }
    }
}
