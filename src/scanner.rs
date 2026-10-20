use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::Semaphore;
use tokio::time::timeout;
use tracing::info;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DiscoveredPrinter {
    pub name: String,
    pub ip_address: String,
    pub port: u16,
    pub printer_type: String,
    pub paper_width: u8,
}

pub struct PrinterScanner;

impl PrinterScanner {
    /// Yerel ağdaki (Subnet) ve USB üzerindeki termal yazıcıları otonom tara
    pub async fn scan_all() -> Vec<DiscoveredPrinter> {
        let mut discovered = Vec::new();

        // 1. Ağ Yazıcılarını Tara (TCP RAW 9100)
        let net_printers = Self::scan_network(Duration::from_millis(600)).await;
        discovered.extend(net_printers);

        // 2. OS / USB Spooler Yazıcılarını Tara
        let usb_printers = Self::scan_os_printers().await;
        discovered.extend(usb_printers);

        discovered
    }

    /// Yerel IP alt ağını (192.168.x.1..=254) paralel async olarak tara
    /// Max 32 eşzamanlı TCP bağlantı ile sınırlı (Semaphore korumalı)
    pub async fn scan_network(probe_timeout: Duration) -> Vec<DiscoveredPrinter> {
        Self::scan_network_with_limit(probe_timeout, 32).await
    }

    /// Configurable concurrency limit for network scanning
    pub async fn scan_network_with_limit(
        probe_timeout: Duration,
        max_concurrent: usize,
    ) -> Vec<DiscoveredPrinter> {
        let local_ip = match local_ip_address::local_ip() {
            Ok(IpAddr::V4(ipv4)) => ipv4,
            _ => return Vec::new(),
        };

        let octets = local_ip.octets();
        let subnet_prefix = format!("{}.{}.{}", octets[0], octets[1], octets[2]);
        info!(
            "Ağdaki termal yazıcılar otonom taranıyor: {}.1 - 254 (Port 9100, max_concurrent={})...",
            subnet_prefix, max_concurrent
        );

        let semaphore = Arc::new(Semaphore::new(max_concurrent));
        let mut tasks = Vec::new();

        for host in 1..=254u32 {
            let ip_str = format!("{}.{}", subnet_prefix, host);
            let port = 9100u16;
            let sem = semaphore.clone();

            tasks.push(tokio::spawn(async move {
                let _permit = sem.acquire().await.expect("semaphore closed");
                let addr_str = format!("{}:{}", ip_str, port);
                if let Ok(addr) = addr_str.parse::<SocketAddr>() {
                    if let Ok(Ok(stream)) = timeout(probe_timeout, TcpStream::connect(addr)).await {
                        drop(stream);
                        return Some(DiscoveredPrinter {
                            name: format!("Termal Ağ Yazıcısı ({})", ip_str),
                            ip_address: ip_str,
                            port,
                            printer_type: "network".to_string(),
                            paper_width: 48,
                        });
                    }
                }
                None
            }));
        }

        let mut results = Vec::new();
        for task in tasks {
            if let Ok(Some(printer)) = task.await {
                info!(
                    "Yeni termal yazıcı otonom keşfedildi: {} -> {}:{}",
                    printer.name, printer.ip_address, printer.port
                );
                results.push(printer);
            }
        }

        results
    }

    /// İşletim sisteminde tanımlı termal yazıcıları bul (macOS CUPS / Windows Spooler)
    pub async fn scan_os_printers() -> Vec<DiscoveredPrinter> {
        let mut list = Vec::new();

        #[cfg(target_os = "macos")]
        {
            use tokio::process::Command;
            if let Ok(output) = Command::new("lpstat").arg("-p").output().await {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines() {
                    // Örn: "printer EPSON_TM_T88V is idle. enabled since..."
                    if line.starts_with("printer ") {
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        if parts.len() >= 2 {
                            let p_name = parts[1];
                            list.push(DiscoveredPrinter {
                                name: p_name.replace('_', " "),
                                ip_address: "USB-SPOOLER".to_string(),
                                port: 0,
                                printer_type: "usb".to_string(),
                                paper_width: 48,
                            });
                        }
                    }
                }
            }
        }

        #[cfg(target_os = "windows")]
        {
            use tokio::process::Command;
            if let Ok(output) = Command::new("powershell")
                .args(["-NoProfile", "-Command", "Get-Printer | Select-Object -ExpandProperty Name"])
                .output()
                .await
            {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines() {
                    let trimmed = line.trim();
                    if !trimmed.is_empty() {
                        list.push(DiscoveredPrinter {
                            name: trimmed.to_string(),
                            ip_address: "WIN-SPOOLER".to_string(),
                            port: 0,
                            printer_type: "usb".to_string(),
                            paper_width: 48,
                        });
                    }
                }
            }
        }

        list
    }
}
