use anyhow::{anyhow, Result};
use reqwest_dav::{types::Auth, types::Depth, Client as DavClient, ClientBuilder};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebDavConfig {
    pub url: String,
    pub username: String,
    pub password: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queue_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoFile {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub modified: String,
}

pub struct WebDavClient {
    client: DavClient,
    base_url: String,
}

impl WebDavClient {
    pub fn new(config: &WebDavConfig) -> Result<Self> {
        let auth = Auth::Basic(config.username.clone(), config.password.clone());

        let client = ClientBuilder::new()
            .set_host(config.url.trim_end_matches('/').to_string())
            .set_auth(auth)
            .build()
            .map_err(|e| anyhow!("Failed to build WebDAV client: {:?}", e))?;

        Ok(Self {
            client,
            base_url: config.url.trim_end_matches('/').to_string(),
        })
    }

    pub async fn list_videos(&self, path: &str) -> Result<Vec<VideoFile>> {
        let items = self
            .client
            .list(path, Depth::Number(1))
            .await
            .map_err(|e| anyhow!("Failed to list files: {:?}", e))?;

        let mut videos = Vec::new();

        for item in items {
            // ListEntity can be File or Folder - get the href/path
            let (href, size, modified) = match &item {
                reqwest_dav::types::list_cmd::ListEntity::File(f) => {
                    (&f.href, f.content_length, Some(&f.last_modified))
                }
                reqwest_dav::types::list_cmd::ListEntity::Folder(_f) => {
                    // Skip folders
                    continue;
                }
            };

            // Skip the base path itself
            if href == path || (href.ends_with('/') && !href[..href.len()-1].ends_with(path)) {
                continue;
            }

            let name = href
                .rsplit('/')
                .next()
                .unwrap_or("")
                .to_string();

            // Filter for video extensions
            if name.ends_with(".mp4")
                || name.ends_with(".mkv")
                || name.ends_with(".mov")
                || name.ends_with(".avi")
                || name.ends_with(".webm")
            {
                videos.push(VideoFile {
                    path: href.clone(),
                    name,
                    size: size as u64,
                    modified: modified.map(|d| d.to_rfc3339()).unwrap_or_default(),
                });
            }
        }

        Ok(videos)
    }

    pub async fn download_file(&self, path: &str) -> Result<bytes::Bytes> {
        let response = self
            .client
            .get(path)
            .await
            .map_err(|e| anyhow!("Failed to download file: {:?}", e))?;

        let data = response
            .bytes()
            .await
            .map_err(|e| anyhow!("Failed to read response body: {:?}", e))?;

        Ok(data)
    }

    pub async fn upload_file(&self, path: &str, data: Vec<u8>) -> Result<()> {
        self.client
            .put(path, data)
            .await
            .map_err(|e| anyhow!("Failed to upload file: {:?}", e))?;

        Ok(())
    }
}

pub async fn list_videos(
    url: &str,
    username: &str,
    password: &str,
    path: &str,
) -> Result<()> {
    let config = WebDavConfig {
        url: url.to_string(),
        username: username.to_string(),
        password: password.to_string(),
        queue_url: None,
    };

    let client = WebDavClient::new(&config)?;
    let videos = client.list_videos(path).await?;

    println!("Found {} videos:", videos.len());
    for video in videos {
        println!("  - {} ({} bytes)", video.name, video.size);
    }

    Ok(())
}
