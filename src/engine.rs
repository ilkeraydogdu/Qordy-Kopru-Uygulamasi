use crate::autostart::AutostartManager;
use crate::cloud_client::CloudApiClient;
use crate::config::ConfigManager;
use crate::dedup::DeduplicationRing;
use crate::escpos::ReceiptGenerator;
use crate::models::{BridgeStatus, LocalPrintRequest, LocalPrintResponse, PrintJob, WsEvent};
use crate::printer::PrinterManager;
use chrono::Local;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use tracing::{info, warn};

pub struct BridgeEngine {
    pub config: Arc<ConfigManager>,
    pub printers: Arc<PrinterManager>,
    pub cloud: Arc<CloudApiClient>,
    pub ws_sender: broadcast::Sender<WsEvent>,
    pub dedup: DeduplicationRing,
    pub jobs_printed: AtomicU64,
    pub jobs_failed: AtomicU64,
    pub start_time: Instant,
    pub is_cloud_online: std::sync::atomic::AtomicBool,
}

impl BridgeEngine {
    pub fn new(
        config: Arc<ConfigManager>,
        printers: Arc<PrinterManager>,
        cloud: Arc<CloudApiClient>,
    ) -> (Arc<Self>, broadcast::Receiver<WsEvent>) {
        let (ws_sender, ws_receiver) = broadcast::channel(100);

        let engine = Arc::new(Self {
            config,
            printers,
            cloud,
            ws_sender,
            dedup: DeduplicationRing::new(4), // 4 saniyelik anti-duplicate koruması
            jobs_printed: AtomicU64::new(0),
            jobs_failed: AtomicU64::new(0),
            start_time: Instant::now(),
            is_cloud_online: std::sync::atomic::AtomicBool::new(false),
        });

        (engine, ws_receiver)
    }

    pub fn get_status(&self) -> BridgeStatus {
        let cfg = self.config.get();
        let local_ip = local_ip_address::local_ip()
            .map(|ip| ip.to_string())
            .unwrap_or_else(|_| "127.0.0.1".to_string());

        BridgeStatus {
            version: "2.5.0".to_string(),
            bridge_id: cfg.bridge_id,
            business_name: cfg.business_name,
            branch_name: cfg.branch_name,
            branch_code: cfg.branch_code,
            address: cfg.address,
            phone: cfg.phone,
            logo_url: cfg.logo_url,
            business_number: cfg.business_number,
            is_configured: cfg.configured && !cfg.api_key.is_empty(),
            is_cloud_connected: self.is_cloud_online.load(Ordering::Relaxed),
            local_ip,
            local_port: cfg.local_port,
            active_printers: self.printers.list().len(),
            jobs_printed: self.jobs_printed.load(Ordering::Relaxed),
            jobs_failed: self.jobs_failed.load(Ordering::Relaxed),
            uptime_seconds: self.start_time.elapsed().as_secs(),
            autostart_enabled: AutostartManager::is_autostart_enabled(),
        }
    }

    pub fn broadcast(&self, event: WsEvent) {
        let _ = self.ws_sender.send(event);
    }

    /// Bir PrintJob'ı hedef yazıcıya yönlendir ve ESC/POS bas
    pub async fn dispatch_job(&self, job: &PrintJob) -> Result<String, anyhow::Error> {
        let cfg = self.config.get();

        // 1. Ekran / Yazıcı Eşleştirmesini Çöz
        let screen_id = job.screen_id.as_deref().unwrap_or("kitchen_main");
        let target_printer_key = cfg
            .screen_assignments
            .get(screen_id)
            .map(|s| s.as_str())
            .unwrap_or("printer-kitchen");

        let printer = self
            .printers
            .get(target_printer_key)
            .or_else(|| self.printers.get_default())
            .ok_or_else(|| anyhow::anyhow!("Hedef yazıcı bulunamadı: {}", target_printer_key))?;

        let printer_name = printer.name().to_string();

        let order_id = job
            .order_id
            .as_deref()
            .or(job.queue_id.as_deref())
            .unwrap_or("0");
        let table_name = job.table_name.as_deref().unwrap_or("-");

        // Idempotency kontrolü - Çifte istekleri ve kağıt israfını engelle
        let fp = DeduplicationRing::generate_fingerprint(
            job.order_id.as_deref().or(job.queue_id.as_deref()),
            job.table_name.as_deref(),
            job.screen_id.as_deref(),
            job.screen_type.as_deref(),
        );

        if !self.dedup.check_and_record(&fp) {
            info!("Mükerrer yazdırma isteği filtrelendi (Duplicate Suppressed) -> Sipariş: #{}", order_id);
            return Ok(format!("{} (Mükerrer İstek Engellendi)", printer_name));
        }

        // 2. Fiş Verisini Üret (ESC/POS)
        let receipt_gen = ReceiptGenerator::new(
            cfg.business_name.clone(),
            cfg.address.clone(),
            cfg.phone.clone(),
        );

        let escpos_bytes = receipt_gen.generate_from_job(job);

        // 3. Yazıcıya Gönder
        info!(
            "Fiş basılıyor -> Yazıcı: [{}] | Masa: {} | Sipariş: #{}",
            printer_name, table_name, order_id
        );

        match printer.print_raw(&escpos_bytes).await {
            Ok(_) => {
                self.jobs_printed.fetch_add(1, Ordering::Relaxed);

                self.broadcast(WsEvent::JobPrinted {
                    order_id: order_id.to_string(),
                    table_name: table_name.to_string(),
                    printer_name: printer_name.clone(),
                    timestamp: Local::now().to_rfc3339(),
                });

                Ok(printer_name)
            }
            Err(e) => {
                self.jobs_failed.fetch_add(1, Ordering::Relaxed);

                self.broadcast(WsEvent::JobFailed {
                    order_id: order_id.to_string(),
                    error: e.to_string(),
                    timestamp: Local::now().to_rfc3339(),
                });

                Err(e)
            }
        }
    }

    /// Mobil Uygulamadan Gelen Yerel Yazdırma İsteğini İşle (LAN Direct Print)
    pub async fn handle_local_print(
        &self,
        req: LocalPrintRequest,
    ) -> Result<LocalPrintResponse, anyhow::Error> {
        let job = PrintJob {
            queue_id: Some(uuid::Uuid::new_v4().to_string()),
            screen_id: req.screen_id.clone(),
            screen_name: Some("Mobil Garson / Kasiyer".to_string()),
            screen_type: Some("kitchen".to_string()),
            order_id: req.order_id.clone(),
            table_name: req.table_name.clone(),
            waiter_name: req.waiter_name.clone(),
            content: req.raw_text.clone(),
            items: req.items,
            total_amount: req.total_amount,
            payment_type: req.payment_type,
            notes: req.notes,
            created_at: Some(Local::now().to_rfc3339()),
        };

        let target_printer = self.dispatch_job(&job).await?;

        Ok(LocalPrintResponse {
            success: true,
            job_id: job.queue_id.unwrap_or_default(),
            message: "Fiş yerel ağ üzerinden anında basıldı".to_string(),
            target_printer,
            printed_at: Local::now().format("%H:%M:%S").to_string(),
        })
    }

    /// Sistem tepsisinden veya yönetimden test fişi tetikleme
    pub async fn trigger_test_print(&self) -> Result<(), anyhow::Error> {
        let p = self
            .printers
            .get_default()
            .or_else(|| self.printers.list().first().cloned())
            .ok_or_else(|| anyhow::anyhow!("Kayıtlı yazıcı bulunamadı"))?;
        let cfg = self.config.get();
        let local_ip = local_ip_address::local_ip()
            .map(|ip| ip.to_string())
            .unwrap_or_else(|_| "127.0.0.1".to_string());
        let receipt_gen = ReceiptGenerator::new(
            cfg.business_name.clone(),
            cfg.address.clone(),
            cfg.phone.clone(),
        );
        let test_bytes = receipt_gen.build_test_receipt(&local_ip, cfg.local_port, p.name());
        p.print_raw(&test_bytes).await?;
        self.broadcast(WsEvent::JobPrinted {
            order_id: "TEST-FİŞİ".to_string(),
            table_name: "Test Masası".to_string(),
            printer_name: p.name().to_string(),
            timestamp: Local::now().to_rfc3339(),
        });
        Ok(())
    }
}

/// Arka Plan Bulut Kuyruk Dinleyici Servisi (Adaptive Poller)
pub fn spawn_cloud_workers(engine: Arc<BridgeEngine>) {
    // 1. Queue Poller
    let eng_poll = engine.clone();
    tokio::spawn(async move {
        info!("Bulut kuyruk dinleyici servisi başlatıldı (Tokio async loop)");
        loop {
            let cfg = eng_poll.config.get();
            if !cfg.configured || cfg.api_key.is_empty() {
                tokio::time::sleep(Duration::from_secs(3)).await;
                continue;
            }

            match eng_poll.cloud.fetch_queue().await {
                Ok(jobs) => {
                    eng_poll.is_cloud_online.store(true, Ordering::Relaxed);
                    let has_jobs = !jobs.is_empty();
                    for job in jobs {
                        let queue_id = job.queue_id.clone().unwrap_or_default();
                        match eng_poll.dispatch_job(&job).await {
                            Ok(_) => {
                                let _ = eng_poll.cloud.update_job_status(&queue_id, "PRINTED", None).await;
                            }
                            Err(e) => {
                                let _ = eng_poll
                                    .cloud
                                    .update_job_status(&queue_id, "FAILED", Some(&e.to_string()))
                                    .await;
                            }
                        }
                    }

                    // İş varsa hızlıca tekrar sor (500ms), yoksa bekle (1.5s)
                    if has_jobs {
                        tokio::time::sleep(Duration::from_millis(500)).await;
                    } else {
                        tokio::time::sleep(Duration::from_secs(cfg.poll_interval_secs.max(1))).await;
                    }
                }
                Err(e) => {
                    eng_poll.is_cloud_online.store(false, Ordering::Relaxed);
                    warn!("Bulut kuyruk çekilemedi: {}. 5sn sonra tekrar denenecek.", e);
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    });

    // 2. Heartbeat Worker (30 saniyede bir canlılık bildirimi)
    let eng_hb = engine.clone();
    tokio::spawn(async move {
        loop {
            let cfg = eng_hb.config.get();
            if cfg.configured && !cfg.api_key.is_empty() {
                match eng_hb.cloud.send_heartbeat().await {
                    Ok(true) => {
                        eng_hb.is_cloud_online.store(true, Ordering::Relaxed);
                    }
                    _ => {
                        eng_hb.is_cloud_online.store(false, Ordering::Relaxed);
                    }
                }
            } else {
                eng_hb.is_cloud_online.store(false, Ordering::Relaxed);
            }
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    });
}
