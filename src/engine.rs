use crate::autostart::AutostartManager;
use crate::cloud_client::CloudApiClient;
use crate::config::ConfigManager;
use crate::dedup::DeduplicationRing;
use crate::escpos::ReceiptGenerator;
use crate::models::{BridgeStatus, LocalPrintRequest, LocalPrintResponse, PrintJob, WsEvent};
use crate::printer::PrinterManager;
use chrono::Local;
use futures_util::stream::{FuturesUnordered, StreamExt};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, Semaphore};
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
    /// Cached local IP address, computed once at startup.
    pub local_ip: String,
    /// Cached ReceiptGenerator built from initial config. Rebuilt when config version changes.
    pub receipt_gen: parking_lot::RwLock<ReceiptGenerator>,
    /// Tracks the config snapshot used to build the cached receipt_gen.
    pub receipt_gen_config_version: std::sync::atomic::AtomicU64,
    /// Global config version counter, incremented on every update.
    pub config_version: std::sync::atomic::AtomicU64,
}

impl BridgeEngine {
    pub fn new(
        config: Arc<ConfigManager>,
        printers: Arc<PrinterManager>,
        cloud: Arc<CloudApiClient>,
    ) -> (Arc<Self>, broadcast::Receiver<WsEvent>) {
        let (ws_sender, ws_receiver) = broadcast::channel(100);

        // Cache local IP once at startup
        let local_ip = local_ip_address::local_ip()
            .map(|ip| ip.to_string())
            .unwrap_or_else(|_| "127.0.0.1".to_string());

        // Build initial ReceiptGenerator from current config
        let cfg = config.get();
        let receipt_gen = ReceiptGenerator::new(
            cfg.business_name.clone(),
            cfg.address.clone(),
            cfg.phone.clone(),
        );

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
            local_ip,
            receipt_gen: parking_lot::RwLock::new(receipt_gen),
            receipt_gen_config_version: std::sync::atomic::AtomicU64::new(0),
            config_version: std::sync::atomic::AtomicU64::new(0),
        });

        (engine, ws_receiver)
    }

    /// Rebuilds the cached ReceiptGenerator if config version changed.
    /// Call this before using `receipt_gen` field directly.
    pub fn ensure_receipt_gen_current(&self) {
        let current_version = self.config_version.load(Ordering::Relaxed);
        let cached_version = self.receipt_gen_config_version.load(Ordering::Relaxed);
        if current_version != cached_version {
            let cfg = self.config.get();
            let new_gen = ReceiptGenerator::new(
                cfg.business_name.clone(),
                cfg.address.clone(),
                cfg.phone.clone(),
            );
            *self.receipt_gen.write() = new_gen;
            self.receipt_gen_config_version.store(current_version, Ordering::Relaxed);
        }
    }

    /// Generate receipt bytes synchronously using the cached generator.
    /// Safe to call from async contexts since no guard is held across await.
    pub fn generate_test_receipt(&self, local_port: u16, printer_name: &str) -> Vec<u8> {
        self.ensure_receipt_gen_current();
        let gen = self.receipt_gen.read();
        gen.build_test_receipt(&self.local_ip, local_port, printer_name)
    }

    /// Generate diagnostic receipt bytes synchronously.
    pub fn generate_diagnostic_receipt(
        &self,
        local_port: u16,
        active_names: &[String],
        uptime_seconds: u64,
        jobs_printed: u64,
    ) -> Vec<u8> {
        self.ensure_receipt_gen_current();
        let gen = self.receipt_gen.read();
        gen.build_diagnostic_receipt(&self.local_ip, local_port, active_names, uptime_seconds, jobs_printed)
    }

    /// Generate label receipt bytes synchronously.
    pub fn generate_label_receipt(
        &self,
        item_name: &str,
        table: &str,
        order_no: &str,
        notes: &str,
    ) -> Vec<u8> {
        self.ensure_receipt_gen_current();
        let gen = self.receipt_gen.read();
        gen.build_label_receipt(item_name, table, order_no, notes)
    }

    /// Returns a read guard for the cached ReceiptGenerator.
    /// Ensures the generator is up-to-date with the current config before returning.
    #[allow(dead_code)]
    pub fn get_receipt_gen(&self) -> parking_lot::RwLockReadGuard<'_, ReceiptGenerator> {
        self.ensure_receipt_gen_current();
        self.receipt_gen.read()
    }

    /// Generate ESC/POS bytes from a print job synchronously.
    pub fn generate_job_receipt(&self, job: &PrintJob) -> Vec<u8> {
        self.ensure_receipt_gen_current();
        let gen = self.receipt_gen.read();
        gen.generate_from_job(job)
    }

    pub fn get_status(&self) -> BridgeStatus {
        let cfg = self.config.get();

        BridgeStatus {
            version: "2.5.0".to_string(),
            bridge_id: cfg.bridge_id.clone(),
            business_name: cfg.business_name.clone(),
            branch_name: cfg.branch_name.clone(),
            branch_code: cfg.branch_code.clone(),
            address: cfg.address.clone(),
            phone: cfg.phone.clone(),
            logo_url: cfg.logo_url.clone(),
            business_number: cfg.business_number.clone(),
            is_configured: cfg.configured && !cfg.api_key.is_empty(),
            is_cloud_connected: self.is_cloud_online.load(Ordering::Relaxed),
            local_ip: self.local_ip.clone(),
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
            job.queue_id.as_deref(),
            job.order_id.as_deref(),
            job.table_name.as_deref(),
            job.screen_id.as_deref(),
            job.screen_type.as_deref(),
        );

        if !self.dedup.check_and_record(&fp) {
            info!("Mükerrer yazdırma isteği filtrelendi (Duplicate Suppressed) -> Sipariş: #{}", order_id);
            return Ok(format!("{} (Mükerrer İstek Engellendi)", printer_name));
        }

        // 2. Fiş Verisini Üret (ESC/POS) — use cached generator (sync, no guard across await)
        let escpos_bytes = self.generate_job_receipt(job);

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
        let test_bytes = self.generate_test_receipt(cfg.local_port, p.name());
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

/// Arka Plan Bulut Kuyruk Dinleyici Servisi (Parallel Poller with Semaphore)
pub fn spawn_cloud_workers(engine: Arc<BridgeEngine>) {
    // 1. Queue Poller — parallelized with max concurrency 4
    let eng_poll = engine.clone();
    tokio::spawn(async move {
        info!("Bulut kuyruk dinleyici servisi başlatıldı (Parallel Poller, max_concurrency=4)");
        let semaphore = Arc::new(Semaphore::new(4));

        loop {
            let cfg = eng_poll.config.get();
            if !cfg.configured || cfg.api_key.is_empty() {
                tokio::time::sleep(Duration::from_secs(3)).await;
                continue;
            }

            let poll_interval = Duration::from_secs(cfg.poll_interval_secs.max(1));

            match eng_poll.cloud.fetch_queue().await {
                Ok(jobs) => {
                    eng_poll.is_cloud_online.store(true, Ordering::Relaxed);
                    let has_jobs = !jobs.is_empty();

                    if has_jobs {
                        // Process jobs in parallel with bounded concurrency
                        let mut futures = FuturesUnordered::new();
                        for job in jobs {
                            let eng = eng_poll.clone();
                            let sem = semaphore.clone();
                            futures.push(tokio::spawn(async move {
                                let _permit = sem.acquire().await.expect("semaphore closed");
                                let queue_id = job.queue_id.clone().unwrap_or_default();
                                let result = eng.dispatch_job(&job).await;
                                (queue_id, result)
                            }));
                        }

                        // Collect results and batch status updates
                        let mut printed_ids: Vec<String> = Vec::new();
                        let mut failed_updates: Vec<(String, String)> = Vec::new();

                        while let Some(task_result) = futures.next().await {
                            if let Ok((queue_id, dispatch_result)) = task_result {
                                match dispatch_result {
                                    Ok(_) => {
                                        printed_ids.push(queue_id);
                                    }
                                    Err(e) => {
                                        failed_updates.push((queue_id, e.to_string()));
                                    }
                                }
                            }
                        }

                        // Batch status updates: send all PRINTED statuses concurrently
                        if !printed_ids.is_empty() {
                            let mut status_futures = FuturesUnordered::new();
                            for qid in printed_ids {
                                let cloud = eng_poll.cloud.clone();
                                status_futures.push(tokio::spawn(async move {
                                    let _ = cloud.update_job_status(&qid, "PRINTED", None).await;
                                }));
                            }
                            while status_futures.next().await.is_some() {}
                        }

                        // Batch status updates: send all FAILED statuses concurrently
                        if !failed_updates.is_empty() {
                            let mut fail_futures = FuturesUnordered::new();
                            for (qid, err_msg) in failed_updates {
                                let cloud = eng_poll.cloud.clone();
                                fail_futures.push(tokio::spawn(async move {
                                    let _ = cloud.update_job_status(&qid, "FAILED", Some(&err_msg)).await;
                                }));
                            }
                            while fail_futures.next().await.is_some() {}
                        }
                    }

                    // Use config-driven poll interval instead of hardcoded values
                    if has_jobs {
                        // After processing a batch, use half the poll interval as fast follow-up
                        tokio::time::sleep(poll_interval / 2).await;
                    } else {
                        tokio::time::sleep(poll_interval).await;
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
