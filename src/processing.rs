use anyhow::Result;
use crate::jobs::{Quadrant, VideoQuadrantSelection};
use std::process::Command;
use tracing::{info, debug};

/// Extract a single frame from a video at a specific position (0-100 percentage)
/// Uses HTTP seeking if the input is a URL
pub fn extract_frame(input: &str, position: u32, output: &str) -> Result<()> {
    extract_frame_with_auth(input, position, output, None, None)
}

/// Extract a single frame with HTTP authentication if needed
/// position: 0 = beginning, 50 = middle, 100 = end
pub fn extract_frame_with_auth(
    input: &str,
    position: u32,
    output: &str,
    username: Option<&str>,
    password: Option<&str>,
) -> Result<()> {
    info!("Extracting frame at {}% from {} to {}", position, input, output);

    let is_http = input.starts_with("http://") || input.starts_with("https://");

    // For position, we use different seeking strategies.
    // -sseof must come BEFORE -i (it's an input option)
    // -ss can come before or after -i (before is faster, after is more compatible)
    let (seek_before, seek_after): (Vec<&str>, Vec<&str>) = match position {
        0 => (vec![], vec!["-ss", "0"]),           // Start
        50 => (vec![], vec!["-ss", "1.5"]),        // Middle (estimate 1.5s for 3s videos)
        100 => (vec!["-sseof", "-0.5"], vec![]),  // End (0.5s before end) - must be before -i
        _ => (vec![], vec!["-ss", "0"]),
    };

    if is_http {
        // For HTTP sources with auth
        let url_with_auth = if let (Some(user), Some(pass)) = (username, password) {
            // URL-encode the username and password to handle special characters
            let encoded_user = urlencoding::encode(user);
            let encoded_pass = urlencoding::encode(pass);
            // Properly inject auth: replace :// with ://user:pass@
            input.replacen("://", &format!("://{}:{}@", encoded_user, encoded_pass), 1)
        } else {
            input.to_string()
        };

        info!("Using URL with auth (credentials hidden)");

        // Build the FFmpeg command
        let mut cmd = Command::new("ffmpeg");
        for arg in &seek_before {
            cmd.arg(arg);
        }
        cmd.arg("-i")
            .arg(&url_with_auth);
        for arg in &seek_after {
            cmd.arg(arg);
        }
        // Set larger output dimensions for better thumbnail quality
        cmd.arg("-vf")
            .arg("scale=1280:-1")  // Scale to 1280px width, maintain aspect ratio
            .arg("-vframes")
            .arg("1")
            .arg("-q:v")
            .arg("5")  // Quality (1-31, lower is better, 5 is good for thumbnails)
            .arg("-y")
            .arg(output);

        let result = cmd.output()?;

        if !result.status.success() {
            let stdout = String::from_utf8_lossy(&result.stdout);
            let stderr = String::from_utf8_lossy(&result.stderr);
            info!("FFmpeg stdout: {}", stdout);
            info!("FFmpeg stderr: {}", stderr);
            return Err(anyhow::anyhow!("FFmpeg failed: {}", stderr));
        }
    } else {
        // For local files
        let mut cmd = Command::new("ffmpeg");
        for arg in &seek_before {
            cmd.arg(arg);
        }
        cmd.arg("-i")
            .arg(input);
        for arg in &seek_after {
            cmd.arg(arg);
        }
        // Set larger output dimensions for better thumbnail quality
        cmd.arg("-vf")
            .arg("scale=1280:-1")  // Scale to 1280px width, maintain aspect ratio
            .arg("-vframes")
            .arg("1")
            .arg("-q:v")
            .arg("5")  // Quality (1-31, lower is better, 5 is good for thumbnails)
            .arg("-y")
            .arg(output);

        let result = cmd.output()?;

        if !result.status.success() {
            let error = String::from_utf8_lossy(&result.stderr);
            return Err(anyhow::anyhow!("FFmpeg failed: {}", error));
        }
    }

    Ok(())
}

/// Extract three frames from a remote video URL directly using HTTP seeking
/// This avoids downloading the entire video file
pub fn extract_preview_frames_from_url(url: &str, output_dir: &str) -> Result<Vec<String>> {
    extract_preview_frames_from_url_with_auth(url, output_dir, None, None)
}

/// Extract three frames from a remote video URL with authentication
pub fn extract_preview_frames_from_url_with_auth(
    url: &str,
    output_dir: &str,
    username: Option<&str>,
    password: Option<&str>,
) -> Result<Vec<String>> {
    std::fs::create_dir_all(output_dir)?;

    // Create a safe filename from the URL
    let filename = url
        .rsplit('/')
        .next()
        .unwrap_or("video")
        .to_string();

    let stem = filename
        .strip_suffix(".mp4")
        .or(filename.strip_suffix(".mkv"))
        .or(filename.strip_suffix(".mov"))
        .or(filename.strip_suffix(".avi"))
        .unwrap_or(&filename)
        .to_string();

    // Sanitize stem for filesystem use
    let stem = stem.replace(|c: char| !c.is_alphanumeric() && c != '-' && c != '_', "_");

    let frames = vec![
        format!("{}/{}_first.jpg", output_dir, stem),
        format!("{}/{}_middle.jpg", output_dir, stem),
        format!("{}/{}_last.jpg", output_dir, stem),
    ];

    // Extract frames directly from URL using HTTP seeking
    // FFmpeg will use range requests to only fetch the needed parts
    extract_frame_with_auth(url, 0, &frames[0], username, password)?;
    extract_frame_with_auth(url, 50, &frames[1], username, password)?;
    extract_frame_with_auth(url, 100, &frames[2], username, password)?;

    Ok(frames)
}

/// Extract three frames from a local video file: beginning, middle, and end
pub fn extract_preview_frames(input: &str, output_dir: &str) -> Result<Vec<String>> {
    std::fs::create_dir_all(output_dir)?;

    let base_name = input
        .rsplit('/')
        .next()
        .unwrap_or(input)
        .rsplit('\\')
        .next()
        .unwrap_or(input);

    let stem = base_name
        .strip_suffix(".mp4")
        .or(base_name.strip_suffix(".mkv"))
        .or(base_name.strip_suffix(".mov"))
        .or(base_name.strip_suffix(".avi"))
        .unwrap_or(base_name);

    let frames = vec![
        format!("{}/{}_first.jpg", output_dir, stem),
        format!("{}/{}_middle.jpg", output_dir, stem),
        format!("{}/{}_last.jpg", output_dir, stem),
    ];

    extract_frame(input, 0, &frames[0])?;
    extract_frame(input, 50, &frames[1])?;
    extract_frame(input, 100, &frames[2])?;

    Ok(frames)
}

/// Get video duration in seconds
pub fn get_video_duration(input: &str) -> Result<f64> {
    let output = Command::new("ffprobe")
        .args([
            "-v", "error",
            "-show_entries", "format=duration",
            "-of", "default=noprint_wrappers=1:nokey=1",
            input,
        ])
        .output()?;

    if !output.status.success() {
        return Err(anyhow::anyhow!("FFprobe failed"));
    }

    let duration = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<f64>()?;

    Ok(duration)
}

/// Get video dimensions
pub fn get_video_dimensions(input: &str) -> Result<(u32, u32)> {
    let output = Command::new("ffprobe")
        .args([
            "-v", "error",
            "-select_streams", "v:0",
            "-show_entries", "stream=width,height",
            "-of", "csv=s=x:p=0",
            input,
        ])
        .output()?;

    if !output.status.success() {
        return Err(anyhow::anyhow!("FFprobe failed"));
    }

    let dimensions = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let parts: Vec<&str> = dimensions.split('x').collect();

    if parts.len() != 2 {
        return Err(anyhow::anyhow!("Invalid dimensions format"));
    }

    let width = parts[0].parse::<u32>()?;
    let height = parts[1].parse::<u32>()?;

    Ok((width, height))
}

/// Calculate crop coordinates for a quadrant
fn calculate_crop(quadrant: Quadrant, width: u32, height: u32) -> (String, u32, u32, u32, u32) {
    let crop_w = width / 2;
    let crop_h = height / 2;

    match quadrant {
        Quadrant::TopLeft => (format!("{}:{}:0:0", crop_w, crop_h), crop_w, crop_h, 0, 0),
        Quadrant::TopRight => (format!("{}:{}:{}:0", crop_w, crop_h, crop_w), crop_w, crop_h, crop_w, 0),
        Quadrant::BottomLeft => (format!("{}:{}:0:{}", crop_w, crop_h, crop_h), crop_w, crop_h, 0, crop_h),
        Quadrant::BottomRight => (format!("{}:{}:{}:{}", crop_w, crop_h, crop_w, crop_h), crop_w, crop_h, crop_w, crop_h),
    }
}

/// Process a video by extracting two quadrants and composing them side by side
pub async fn process_video(input: &str, output: &str) -> Result<()> {
    info!("Processing video: {} -> {}", input, output);

    // For now, use default quadrant selection
    let selection = VideoQuadrantSelection {
        presentation: Quadrant::TopLeft,
        slides: Quadrant::TopRight,
    };

    process_video_with_selection(input, output, &selection).await
}

/// Process a video with specific quadrant selection
pub async fn process_video_with_selection(
    input: &str,
    output: &str,
    selection: &VideoQuadrantSelection,
) -> Result<()> {
    info!("Processing video with selection: {:?} -> {:?}", selection.presentation, selection.slides);

    // Get crop coordinates for the selected quadrants
    let pres_crop = quadrant_crop(&selection.presentation);
    let speaker_crop = quadrant_crop(&selection.slides);

    // Background image path
    let bg_image = "./gpc-bg.png";

    // Build filter complex matching the worker's logic:
    // 1. Scale background to 2560x1440
    // 2. Crop presentation quadrant and scale to 1920x1080
    // 3. Crop speaker/slides quadrant and scale to 320px height (width auto)
    // 4. Overlay presentation centered on background
    // 5. Overlay speaker in bottom-right corner
    let filter = format!(
        "[1:v]scale=2560:1440[bg]; \
         [0:v]crop={}[pres_cropped]; \
         [pres_cropped]scale=1920:1080[pres]; \
         [0:v]crop={}[speaker_raw]; \
         [speaker_raw]scale=-1:320[speaker]; \
         [pres]scale=1920:1080[pres_s]; \
         [bg][pres_s]overlay=(W-w)/2:(H-h)/2[base]; \
         [base][speaker]overlay=x=W-w-40:y=H-h-40[outv]",
        pres_crop, speaker_crop
    );

    debug!("Filter complex: {}", filter);

    let output_result = Command::new("ffmpeg")
        .args([
            "-y",                       // Overwrite output
            "-i", input,                // Input video
            "-i", bg_image,             // Background image
            "-filter_complex", &filter, // Video processing
            "-map", "[outv]",           // Use processed video
            "-map", "0:a?",             // Copy audio if present
            "-c:v", "libx264",          // Video codec
            "-crf", "18",               // Quality
            "-preset", "veryfast",      // Encoding speed
            "-threads", "0",            // Use all threads
            "-c:a", "copy",             // Copy audio
            output,
        ])
        .output()?;

    if !output_result.status.success() {
        let error = String::from_utf8_lossy(&output_result.stderr);
        return Err(anyhow::anyhow!("FFmpeg processing failed: {}", error));
    }

    info!("Video processing completed successfully");
    Ok(())
}

fn quadrant_crop(q: &Quadrant) -> String {
    // Video is 3840x2160 (4K), divided into 4 quadrants of 1920x1080 each
    // We apply a 4px offset to trim borders from each quadrant
    match q {
        Quadrant::TopLeft => "1912:1072:4:4".to_string(),
        Quadrant::TopRight => "1912:1072:1924:4".to_string(),
        Quadrant::BottomLeft => "1912:1072:4:1084".to_string(),
        Quadrant::BottomRight => "1912:1072:1924:1084".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_crop() {
        let (crop, w, h, x, y) = calculate_crop(Quadrant::TopLeft, 1920, 1080);
        assert_eq!(crop, "960:540:0:0");
        assert_eq!(w, 960);
        assert_eq!(h, 540);
        assert_eq!(x, 0);
        assert_eq!(y, 0);

        let (crop, w, h, x, y) = calculate_crop(Quadrant::TopRight, 1920, 1080);
        assert_eq!(crop, "960:540:960:0");
        assert_eq!(x, 960);

        let (crop, _, _, _, _) = calculate_crop(Quadrant::BottomLeft, 1920, 1080);
        assert_eq!(crop, "960:540:0:540");

        let (crop, _, _, _, _) = calculate_crop(Quadrant::BottomRight, 1920, 1080);
        assert_eq!(crop, "960:540:960:540");
    }
}
