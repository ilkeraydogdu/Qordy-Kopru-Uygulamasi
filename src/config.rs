use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use parking_lot::RwLock;
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrinterConfig {
    pub id: String,
    pub name: String,
    pub printer_type: String, // "network", "usb", "mock"
    pub ip_address: Option<String>,
    pub port: Option<u16>,
    pub paper_width: Option<u8>, // 80 or 58 (default 48 characters for 80mm)
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeConfig {
    pub configured: bool,
    pub bridge_id: String,
    pub config_code: String,
    pub api_key: String,
    pub api_base_url: String,
    pub business_id: String,
    pub business_name: String,
    #[serde(default = "default_branch_name")]
    pub branch_name: String,
    #[serde(default = "default_branch_code")]
    pub branch_code: String,
    pub address: String,
    pub phone: String,
    #[serde(default)]
    pub logo_url: Option<String>,
    #[serde(default)]
    pub business_number: Option<String>,
    pub local_port: u16,
    pub poll_interval_secs: u64,
    pub printers: Vec<PrinterConfig>,
    pub screen_assignments: HashMap<String, String>, // screen_id -> printer_id / printer_name
}

fn default_branch_name() -> String {
    String::new()
}

fn default_branch_code() -> String {
    String::new()
}

impl Default for BridgeConfig {
    fn default() -> Self {
        Self {
            configured: false,
            bridge_id: String::new(),
            config_code: String::new(),
            api_key: String::new(),
            api_base_url: "https://qordy.com/pb".to_string(),
            business_id: String::new(),
            business_name: String::new(),
            branch_name: String::new(),
            branch_code: String::new(),
            address: String::new(),
            phone: String::new(),
            logo_url: None,
            business_number: None,
            local_port: 8089,
            poll_interval_secs: 1,
            printers: Vec::new(),
            screen_assignments: HashMap::new(),
        }
    }
}

pub struct ConfigManager {
    path: PathBuf,
    data: Arc<RwLock<Arc<BridgeConfig>>>,
}

impl ConfigManager {
    pub fn new<P: AsRef<Path>>(config_path: P) -> Self {
        let path = config_path.as_ref().to_path_buf();
        let config = if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(content) => match serde_json::from_str::<BridgeConfig>(&content) {
                    Ok(parsed) => {
                        info!("Yapılandırma yüklendi: {:?}", path);
                        parsed
                    }
                    Err(e) => {
                        warn!("Yapılandırma parse edilemedi: {}. Varsayılan kullanılıyor.", e);
                        BridgeConfig::default()
                    }
                },
                Err(e) => {
                    warn!("Yapılandırma dosyası okunamadı: {}. Varsayılan kullanılıyor.", e);
                    BridgeConfig::default()
                }
            }
        } else {
            let default_cfg = BridgeConfig::default();
            if let Ok(json) = serde_json::to_string_pretty(&default_cfg) {
                let _ = std::fs::write(&path, json);
                info!("Yeni varsayılan yapılandırma oluşturuldu: {:?}", path);
            }
            default_cfg
        };

        Self {
            path,
            data: Arc::new(RwLock::new(Arc::new(config))),
        }
    }

    /// Returns a cheap Arc clone (pointer copy only, no deep clone of BridgeConfig).
    pub fn get(&self) -> Arc<BridgeConfig> {
        self.data.read().clone()
    }

    pub fn update<F>(&self, mut mutator: F) -> Result<Arc<BridgeConfig>, anyhow::Error>
    where
        F: FnMut(&mut BridgeConfig),
    {
        let updated = {
            let mut write_guard = self.data.write();
            // Clone the inner config, mutate, then replace the Arc atomically
            let mut cfg = (**write_guard).clone();
            mutator(&mut cfg);
            let new_arc = Arc::new(cfg);
            *write_guard = new_arc.clone();
            new_arc
        };

        self.save()?;
        Ok(updated)
    }

    pub fn save(&self) -> Result<(), anyhow::Error> {
        let json = {
            let read_guard = self.data.read();
            serde_json::to_string_pretty(&**read_guard)?
        };
        std::fs::write(&self.path, json)?;
        info!("Yapılandırma diske kaydedildi: {:?}", self.path);
        Ok(())
    }
}
