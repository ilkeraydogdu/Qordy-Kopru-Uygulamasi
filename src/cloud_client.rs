#![allow(dead_code)]
use crate::config::ConfigManager;
use crate::models::{
    HeartbeatRequest, HeartbeatResponse, PrintJob, QueueResponse, RegisterRequest,
    RegisterResponse, Screen, ScreensResponse, UpdateStatusRequest,
};
use reqwest::Client;
use std::sync::Arc;
use std::time::Duration;
use tracing::info;

pub struct CloudApiClient {
    client: Client,
    config: Arc<ConfigManager>,
}

impl CloudApiClient {
    pub fn new(config: Arc<ConfigManager>) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(6))
            .user_agent("QORDY-Bridge-Rust/2.5.0")
            .tcp_nodelay(true)
            .tcp_keepalive(Duration::from_secs(30))
            .pool_idle_timeout(Duration::from_secs(90))
            .pool_max_idle_per_host(8)
            .build()
            .unwrap_or_default();

        Self { client, config }
    }

    fn base_url(&self) -> String {
        let cfg = self.config.get();
        cfg.api_base_url.trim_end_matches('/').to_string()
    }

    /// Köprüyü Web Paneli Yapılandırma Koduyla Kaydet
    pub async fn register(&self, config_code: &str) -> Result<RegisterResponse, anyhow::Error> {
        let url = format!("{}/register", self.base_url());
        let current_cfg = self.config.get();

        let req = RegisterRequest {
            bridge_id: current_cfg.bridge_id,
            config_code: config_code.to_string(),
            device_name: std::env::var("HOSTNAME")
                .or_else(|_| std::env::var("COMPUTERNAME"))
                .unwrap_or_else(|_| "Qordy-Bridge-Host".to_string()),
            version: "2.5.0".to_string(),
            os: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
        };

        info!("Buluta kayıt isteği gönderiliyor: {}", url);
        let resp = self.client.post(&url).json(&req).send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Kayıt hatası HTTP {}: {}", status, body));
        }

        let result: RegisterResponse = resp.json().await?;

        if result.success {
            info!("Köprü başarıyla buluta kaydedildi!");
            let _ = self.config.update(|cfg| {
                if let Some(b_id) = &result.bridge_id {
                    cfg.bridge_id = b_id.clone();
                }
                if let Some(key) = &result.api_key {
                    cfg.api_key = key.clone();
                }
                if let Some(bus_id) = &result.business_id {
                    cfg.business_id = bus_id.clone();
                }
                if let Some(bus_name) = &result.business_name {
                    cfg.business_name = bus_name.clone();
                }
                if let Some(br_name) = &result.branch_name {
                    cfg.branch_name = br_name.clone();
                }
                if let Some(br_code) = &result.branch_code {
                    cfg.branch_code = br_code.clone();
                }
                if let Some(addr) = &result.address {
                    cfg.address = addr.clone();
                }
                if let Some(ph) = &result.phone {
                    cfg.phone = ph.clone();
                }
                if let Some(logo) = &result.logo_url {
                    cfg.logo_url = Some(logo.clone());
                }
                if let Some(num) = &result.business_number {
                    cfg.business_number = Some(num.clone());
                }
                cfg.config_code = config_code.to_string();
                cfg.configured = true;
            });

            // Arka planda logoyu indirip önbelleğe al
            if let Some(logo_url) = &result.logo_url {
                let client_clone = self.client.clone();
                let url_clone = logo_url.clone();
                tokio::spawn(async move {
                    if let Ok(resp) = client_clone.get(&url_clone).send().await {
                        if resp.status().is_success() {
                            if let Ok(bytes) = resp.bytes().await {
                                let _ = std::fs::create_dir_all("assets");
                                let _ = std::fs::write("assets/caddecafe_logo.webp", &bytes);
                                let _ = std::fs::write("assets/business_logo.webp", &bytes);
                                info!("İşletme logosu başarıyla indirildi ve önbelleğe alındı.");
                            }
                        }
                    }
                });
            }
        }

        Ok(result)
    }

    /// Heartbeat Gönderimi
    pub async fn send_heartbeat(&self) -> Result<bool, anyhow::Error> {
        let cfg = self.config.get();
        if !cfg.configured || cfg.api_key.is_empty() {
            return Ok(false);
        }

        let url = format!("{}/heartbeat", self.base_url());
        let req = HeartbeatRequest {
            bridge_id: cfg.bridge_id,
            api_key: cfg.api_key,
        };

        let resp = self.client.post(&url).json(&req).send().await?;
        if resp.status().is_success() {
            let body: HeartbeatResponse = resp.json().await?;
            return Ok(body.success);
        }
        Ok(false)
    }

    /// Hazırlık Ekranlarını Getir
    pub async fn fetch_screens(&self) -> Result<Vec<Screen>, anyhow::Error> {
        let cfg = self.config.get();
        let url = format!("{}/screens", self.base_url());

        let resp = self
            .client
            .get(&url)
            .query(&[("bridge_id", &cfg.bridge_id), ("api_key", &cfg.api_key)])
            .send()
            .await?;

        if resp.status().is_success() {
            let data: ScreensResponse = resp.json().await?;
            if let Some(screens) = data.screens {
                return Ok(screens);
            }
        }
        Ok(Vec::new())
    }

    /// Bulut Yazdırma Kuyruğunu Çek (Queue Polling)
    pub async fn fetch_queue(&self) -> Result<Vec<PrintJob>, anyhow::Error> {
        let cfg = self.config.get();
        if !cfg.configured || cfg.api_key.is_empty() {
            return Ok(Vec::new());
        }

        let url = format!("{}/queue", self.base_url());
        let body = serde_json::json!({
            "bridge_id": cfg.bridge_id,
            "api_key": cfg.api_key,
            "limit": 20
        });

        let resp = self.client.post(&url).json(&body).send().await?;
        if resp.status().is_success() {
            let queue_resp: QueueResponse = resp.json().await?;
            if let Some(jobs) = queue_resp.jobs {
                return Ok(jobs);
            }
        }
        Ok(Vec::new())
    }

    /// Fiş Basım Durumunu Buluta Bildir (PRINTED / FAILED)
    pub async fn update_job_status(
        &self,
        queue_id: &str,
        status: &str,
        error_msg: Option<&str>,
    ) -> Result<bool, anyhow::Error> {
        let cfg = self.config.get();
        let url = format!("{}/update-status", self.base_url());

        let req = UpdateStatusRequest {
            bridge_id: cfg.bridge_id,
            api_key: cfg.api_key,
            queue_id: queue_id.to_string(),
            status: status.to_string(),
            error_message: error_msg.map(|s| s.to_string()),
        };

        let resp = self.client.post(&url).json(&req).send().await?;
        Ok(resp.status().is_success())
    }
}
