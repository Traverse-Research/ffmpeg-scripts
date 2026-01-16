use anyhow::Result;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

const HETZNER_API_BASE: &str = "https://api.hetzner.cloud/v1";

#[derive(Debug, Serialize, Deserialize)]
pub struct ServerConfig {
    pub name: String,
    pub server_type: String,
    pub image: String,
    pub location: String,
    pub ssh_keys: Vec<String>,
    pub user_data: String,
    pub labels: Vec<(String, String)>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            name: "ffmpeg-worker".to_string(),
            server_type: "cpx21".to_string(), // 4 CPU, 8 GB RAM
            image: "ubuntu-24.04".to_string(),
            location: "fsn1".to_string(), // Falkenstein
            ssh_keys: vec![],
            user_data: String::new(),
            labels: vec![("worker".to_string(), "ffmpeg-gpc".to_string())],
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Server {
    pub id: u64,
    pub name: String,
    pub status: String,
    pub public_net: PublicNet,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PublicNet {
    pub ipv4: Ipv4,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IpAddress {
    pub ip: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Ipv4 {
    pub ip: String,
}

#[derive(Debug, Serialize)]
struct CreateServerRequest {
    name: String,
    server_type: String,
    image: String,
    location: String,
    ssh_keys: Option<Vec<String>>,
    user_data: Option<String>,
    labels: Option<Vec<Label>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Label {
    key: String,
    value: String,
}

#[derive(Debug, Deserialize)]
struct CreateServerResponse {
    server: ServerData,
}

#[derive(Debug, Deserialize)]
struct ServerData {
    server: Server,
}

#[derive(Debug, Deserialize)]
struct ErrorResponse {
    error: ErrorDetail,
}

#[derive(Debug, Deserialize)]
struct ErrorDetail {
    message: String,
}

pub struct HetznerClient {
    api_token: String,
    client: reqwest::Client,
}

impl HetznerClient {
    pub fn new(api_token: String) -> Self {
        Self {
            api_token,
            client: reqwest::Client::new(),
        }
    }

    pub async fn create_server(&self, config: &ServerConfig) -> Result<Server> {
        let url = format!("{}/servers", HETZNER_API_BASE);

        let labels = if config.labels.is_empty() {
            None
        } else {
            Some(
                config
                    .labels
                    .iter()
                    .map(|(k, v)| Label {
                        key: k.clone(),
                        value: v.clone(),
                    })
                    .collect(),
            )
        };

        let ssh_keys = if config.ssh_keys.is_empty() {
            None
        } else {
            Some(config.ssh_keys.clone())
        };

        let user_data = if config.user_data.is_empty() {
            None
        } else {
            Some(config.user_data.clone())
        };

        let payload = CreateServerRequest {
            name: config.name.clone(),
            server_type: config.server_type.clone(),
            image: config.image.clone(),
            location: config.location.clone(),
            ssh_keys,
            user_data,
            labels,
        };

        debug!("Creating server: {}", config.name);

        let response = self
            .client
            .post(&url)
            .header(AUTHORIZATION, format!("Bearer {}", self.api_token))
            .header(CONTENT_TYPE, "application/json")
            .json(&payload)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to send request: {}", e))?;

        let status = response.status();

        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Failed to create server: {} - {}",
                status,
                error_text
            ));
        }

        let result: CreateServerResponse = response
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to parse response: {}", e))?;

        info!(
            "Server created: {} (ID: {}, IP: {})",
            result.server.server.name, result.server.server.id, result.server.server.public_net.ipv4.ip
        );

        Ok(result.server.server)
    }

    pub async fn delete_server(&self, id: u64) -> Result<()> {
        let url = format!("{}/servers/{}", HETZNER_API_BASE, id);

        info!("Deleting server: {}", id);

        let response = self
            .client
            .delete(&url)
            .header(AUTHORIZATION, format!("Bearer {}", self.api_token))
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to send request: {}", e))?;

        let status = response.status();

        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Failed to delete server: {} - {}",
                status,
                error_text
            ));
        }

        info!("Server {} deleted", id);
        Ok(())
    }

    pub async fn list_servers(&self) -> Result<Vec<Server>> {
        let url = format!("{}/servers", HETZNER_API_BASE);

        let response = self
            .client
            .get(&url)
            .header(AUTHORIZATION, format!("Bearer {}", self.api_token))
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to send request: {}", e))?;

        let status = response.status();

        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Failed to list servers: {} - {}",
                status,
                error_text
            ));
        }

        #[derive(Debug, Deserialize)]
        struct ListServersResponse {
            servers: Vec<ServerData>,
        }

        let result: ListServersResponse = response
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to parse response: {}", e))?;

        Ok(result.servers.into_iter().map(|s| s.server).collect())
    }

    pub async fn get_server(&self, id: u64) -> Result<Server> {
        let url = format!("{}/servers/{}", HETZNER_API_BASE, id);

        let response = self
            .client
            .get(&url)
            .header(AUTHORIZATION, format!("Bearer {}", self.api_token))
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to send request: {}", e))?;

        let status = response.status();

        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Failed to get server: {} - {}",
                status,
                error_text
            ));
        }

        let result: ServerData = response
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to parse response: {}", e))?;

        Ok(result.server)
    }

    /// Generate cloud-init user data for worker setup
    pub fn worker_cloud_init(queue_url: &str, binary_url: &str, bg_image_url: &str) -> String {
        format!(
            r#"#cloud-config
package_update: true
package_upgrade: true
packages:
  - ffmpeg
  - wget

runcmd:
  - wget -O /root/gpc-bg.png {bg_image_url}
  - wget -O /tmp/worker {binary_url}
  - chmod +x /tmp/worker
  - /tmp/worker worker --queue-url {queue_url}

final_message: "FFmpeg worker is ready!"
"#
        )
    }
}

/// Cloud-init config with SSH key access
pub fn worker_cloud_init_with_ssh(
    queue_url: &str,
    binary_url: &str,
    bg_image_url: &str,
    ssh_public_key: &str,
) -> String {
    format!(
        r#"#cloud-config
package_update: true
package_upgrade: true
packages:
  - ffmpeg
  - wget

ssh_authorized_keys:
  - {ssh_public_key}

runcmd:
  - wget -O /root/gpc-bg.png {bg_image_url}
  - wget -O /tmp/worker {binary_url}
  - chmod +x /tmp/worker
  - /tmp/worker worker --queue-url {queue_url}

final_message: "FFmpeg worker is ready!"
"#
    )
}

pub async fn provision_worker(
    hetzner_token: &str,
    queue_url: &str,
    binary_url: &str,
    bg_image_url: &str,
    name: Option<String>,
) -> Result<String> {
    let client = HetznerClient::new(hetzner_token.to_string());

    let name = name.unwrap_or_else(|| {
        format!("ffmpeg-worker-{}", chrono::Utc::now().format("%Y%m%d-%H%M%S"))
    });

    let user_data = HetznerClient::worker_cloud_init(queue_url, binary_url, bg_image_url);

    let config = ServerConfig {
        name,
        server_type: "cpx21".to_string(),
        image: "ubuntu-24.04".to_string(),
        location: "fsn1".to_string(),
        user_data,
        ..Default::default()
    };

    let server = client.create_server(&config).await?;
    Ok(server.public_net.ipv4.ip)
}
