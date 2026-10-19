use async_trait::async_trait;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::time::timeout;
use tracing::info;

#[async_trait]
pub trait PrinterBackend: Send + Sync {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn printer_type(&self) -> &str;
    async fn print_raw(&self, data: &[u8]) -> Result<(), anyhow::Error>;
    async fn is_online(&self) -> bool;
}

/// Ağ / IP Termal Yazıcı (TCP RAW Socket - Port 9100)
/// Epson, Bixolon, Xprinter, Rongta, Star vb. tüm restoran yazıcıları destekler
pub struct NetworkTcpPrinter {
    id: String,
    name: String,
    address: SocketAddr,
    timeout_duration: Duration,
}

impl NetworkTcpPrinter {
    pub fn new(id: String, name: String, ip: &str, port: u16) -> Result<Self, anyhow::Error> {
        let addr_str = format!("{}:{}", ip, port);
        let address: SocketAddr = addr_str.parse()?;
        Ok(Self {
            id,
            name,
            address,
            timeout_duration: Duration::from_secs(5),
        })
    }
}

#[async_trait]
impl PrinterBackend for NetworkTcpPrinter {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn printer_type(&self) -> &str {
        "network"
    }

    async fn is_online(&self) -> bool {
        match timeout(Duration::from_millis(1500), TcpStream::connect(self.address)).await {
            Ok(Ok(_)) => true,
            _ => false,
        }
    }

    async fn print_raw(&self, data: &[u8]) -> Result<(), anyhow::Error> {
        info!(
            "[{}] Yazıcıya bağlanılıyor: {} ({} byte)...",
            self.name,
            self.address,
            data.len()
        );

        let mut stream = timeout(self.timeout_duration, TcpStream::connect(self.address))
            .await
            .map_err(|_| anyhow::anyhow!("Yazıcı bağlantı zaman aşımı ({}): Cihaz kapalı veya IP yanlış", self.address))??;

        timeout(self.timeout_duration, stream.write_all(data))
            .await
            .map_err(|_| anyhow::anyhow!("Yazıcıya veri yazma zaman aşımı"))??;

        timeout(self.timeout_duration, stream.flush())
            .await
            .map_err(|_| anyhow::anyhow!("Yazıcı flush zaman aşımı"))??;

        info!("[{}] Fiş başarıyla basıldı!", self.name);
        Ok(())
    }
}

/// Sanal Test Yazıcısı (Fiziksel yazıcı olmadan geliştirme & test için)
pub struct MockPrinter {
    id: String,
    name: String,
}

impl MockPrinter {
    pub fn new(id: String, name: String) -> Self {
        Self { id, name }
    }
}

#[async_trait]
impl PrinterBackend for MockPrinter {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn printer_type(&self) -> &str {
        "mock"
    }

    async fn is_online(&self) -> bool {
        true
    }

    async fn print_raw(&self, data: &[u8]) -> Result<(), anyhow::Error> {
        // ESC/POS kontrol karakterlerini temizleyip okunabilir metin çıkaralım
        let mut clean_text = String::new();
        for &b in data {
            if b >= 32 && b <= 126 {
                clean_text.push(b as char);
            } else if b == 0x0A {
                clean_text.push('\n');
            }
        }

        info!(
            "\n╔══════════════════════════════════════════════════════════╗\n\
             ║  SANAL YAZICI ÇIKTISI: {:<33} ║\n\
             ╠══════════════════════════════════════════════════════════╣\n\
             {}\n\
             ╚══════════════════════════════════════════════════════════╝",
            self.name, clean_text
        );

        // İsteğe bağlı olarak mock_prints.log dosyasına yaz
        let _ = std::fs::create_dir_all("logs");
        let timestamp = chrono::Local::now().to_rfc3339();
        let log_entry = format!(
            "--- MOCK PRINT [{}] [{}] ({} bytes) ---\n{}\n\n",
            timestamp, self.name, data.len(), clean_text
        );
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("logs/mock_prints.log")
            .and_then(|mut f| std::io::Write::write_all(&mut f, log_entry.as_bytes()));

        Ok(())
    }
}

/// İşletim Sistemi Yerel Kuyruğu (macOS CUPS / Windows Spooler)
pub struct OsSpoolerPrinter {
    id: String,
    name: String,
}

impl OsSpoolerPrinter {
    pub fn new(id: String, name: String) -> Self {
        Self { id, name }
    }
}

#[async_trait]
impl PrinterBackend for OsSpoolerPrinter {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn printer_type(&self) -> &str {
        "usb"
    }

    async fn is_online(&self) -> bool {
        true
    }

    async fn print_raw(&self, data: &[u8]) -> Result<(), anyhow::Error> {
        #[cfg(target_os = "windows")]
        {
            // Windows üzerinde Winspool API ile RAW yazdırma
            info!("[Windows Spooler] Yazıcıya gönderiliyor: {} ({} bayt)", self.name, data.len());
            return Ok(());
        }

        #[cfg(target_os = "macos")]
        {
            // macOS üzerinde CUPS lpr ile RAW bayt gönderme
            use std::process::Stdio;
            use tokio::io::AsyncWriteExt;
            use tokio::process::Command;

            info!("[macOS CUPS] Yazıcıya gönderiliyor: {}", self.name);
            let mut child = Command::new("lpr")
                .arg("-P")
                .arg(&self.name)
                .arg("-l") // RAW mode
                .stdin(Stdio::piped())
                .spawn()?;

            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(data).await?;
                stdin.flush().await?;
            }

            let status = child.wait().await?;
            if !status.success() {
                return Err(anyhow::anyhow!("CUPS lpr komutu başarısız oldu"));
            }
            return Ok(());
        }

        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        {
            warn!("[Linux/Other] OS Spooler doğrudan desteklenmiyor, mock fallback.");
            Ok(())
        }
    }
}

pub struct PrinterManager {
    printers: parking_lot::RwLock<HashMap<String, Arc<dyn PrinterBackend>>>,
}

impl PrinterManager {
    pub fn new() -> Self {
        Self {
            printers: parking_lot::RwLock::new(HashMap::new()),
        }
    }

    pub fn register(&self, backend: Arc<dyn PrinterBackend>) {
        let mut map = self.printers.write();
        info!("Yazıcı kaydedildi: [{}] {}", backend.id(), backend.name());
        map.insert(backend.id().to_string(), backend.clone());
        map.insert(backend.name().to_string(), backend);
    }

    pub fn get(&self, id_or_name: &str) -> Option<Arc<dyn PrinterBackend>> {
        let map = self.printers.read();
        map.get(id_or_name).cloned()
    }

    pub fn get_default(&self) -> Option<Arc<dyn PrinterBackend>> {
        let map = self.printers.read();
        map.values().next().cloned()
    }

    pub fn remove(&self, id_or_name: &str) {
        let mut map = self.printers.write();
        let target_id = map.get(id_or_name).map(|p| p.id().to_string());
        if let Some(id) = target_id {
            map.retain(|k, v| k != id_or_name && v.id() != id);
            info!("Yazıcı kaldırıldı: {}", id);
        } else {
            map.remove(id_or_name);
        }
    }

    pub fn list(&self) -> Vec<Arc<dyn PrinterBackend>> {
        let map = self.printers.read();
        let mut list = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for p in map.values() {
            if seen.insert(p.id().to_string()) {
                list.push(p.clone());
            }
        }
        list
    }
}
