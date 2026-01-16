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
            server_type: "cpx11".to_string(), // 4 CPU, 8 GB RAM
            image: "ubuntu-24.04".to_string(),
            location: "nbg1".to_string(), // Falkenstein
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

#[derive(Debug)]
pub struct ServerType {
    pub name: String,
    pub cores: u32,
    pub memory: u32,
    pub disk: u32,
    pub locations: Vec<String>,
}

#[derive(Debug)]
pub struct Datacenter {
    pub name: String,
    pub location: String,
    pub server_types: Vec<String>,
}

#[derive(Debug, Serialize)]
struct CreateServerRequest {
    name: String,
    server_type: String,
    image: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ssh_keys: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    user_data: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    labels: Option<std::collections::HashMap<String, String>>,
}

#[derive(Debug, Deserialize)]
struct CreateServerResponse {
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
            Some(config.labels.iter().cloned().collect())
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

        let location = if config.location.is_empty() {
            None
        } else {
            Some(config.location.clone())
        };

        let payload = CreateServerRequest {
            name: config.name.clone(),
            server_type: config.server_type.clone(),
            image: config.image.clone(),
            location,
            ssh_keys,
            user_data,
            labels,
        };

        debug!("Creating server: {} with type: {}, location: {:?}", config.name, config.server_type, &payload.location);
        debug!("Request payload: {:?}", serde_json::to_string(&payload));

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
            result.server.name, result.server.id, result.server.public_net.ipv4.ip
        );

        Ok(result.server)
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
            servers: Vec<Server>,
        }

        let result: ListServersResponse = response
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to parse response: {}", e))?;

        Ok(result.servers)
    }

    pub async fn list_server_types(&self) -> Result<Vec<ServerType>> {
        let url = format!("{}/server_types", HETZNER_API_BASE);

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
                "Failed to list server types: {} - {}",
                status,
                error_text
            ));
        }

        #[derive(Debug, Deserialize)]
        struct ServerTypeResponse {
            name: String,
            cores: u32,
            memory: f64,
            disk: u32,
            prices: Vec<PriceInfo>,
        }

        #[derive(Debug, Deserialize)]
        struct PriceInfo {
            location: String,
        }

        #[derive(Debug, Deserialize)]
        struct ListServerTypesResponse {
            server_types: Vec<ServerTypeResponse>,
        }

        let result: ListServerTypesResponse = response
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to parse response: {}", e))?;

        Ok(result
            .server_types
            .into_iter()
            .map(|st| ServerType {
                name: st.name,
                cores: st.cores,
                memory: st.memory as u32,
                disk: st.disk,
                locations: st.prices.into_iter().map(|p| p.location).collect(),
            })
            .collect())
    }

    pub async fn list_datacenters(&self) -> Result<Vec<Datacenter>> {
        let url = format!("{}/datacenters", HETZNER_API_BASE);

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
                "Failed to list datacenters: {} - {}",
                status,
                error_text
            ));
        }

        #[derive(Debug, Deserialize)]
        struct Location {
            name: String,
        }

        #[derive(Debug, Deserialize)]
        struct ServerTypesAvailable {
            available: Vec<u64>,
            available_for_migration: Vec<u64>,
            supported: Vec<u64>,
        }

        #[derive(Debug, Deserialize)]
        struct DatacenterResponse {
            name: String,
            location: Location,
            server_types: ServerTypesAvailable,
        }

        #[derive(Debug, Deserialize)]
        struct ListDatacentersResponse {
            datacenters: Vec<DatacenterResponse>,
        }

        let result: ListDatacentersResponse = response
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to parse response: {}", e))?;

        Ok(result
            .datacenters
            .into_iter()
            .map(|dc| Datacenter {
                name: dc.name,
                location: dc.location.name,
                server_types: dc.server_types.available.iter().map(|id| id.to_string()).collect(),
            })
            .collect())
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

        #[derive(Debug, Deserialize)]
        struct GetServerResponse {
            server: Server,
        }

        let result: GetServerResponse = response
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
        server_type: "ccx23".to_string(), // 4 dedicated vCPUs, 16GB RAM
        image: "ubuntu-24.04".to_string(),
        location: "nbg1".to_string(), // Nuremberg, Germany
        user_data,
        ..Default::default()
    };

    let server = client.create_server(&config).await?;
    Ok(server.public_net.ipv4.ip)
}
