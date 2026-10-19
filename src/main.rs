mod autostart;
mod cloud_client;
mod config;
mod dedup;
mod engine;
mod escpos;
mod local_server;
mod models;
mod printer;
mod scanner;

use cloud_client::CloudApiClient;
use config::ConfigManager;
use engine::{spawn_cloud_workers, BridgeEngine};
use local_server::LocalServer;
use printer::{MockPrinter, NetworkTcpPrinter, OsSpoolerPrinter, PrinterManager};
use std::sync::Arc;
use tracing::{error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use tao::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};
use wry::WebViewBuilder;
use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    Icon, TrayIconBuilder, TrayIconEvent,
};

#[cfg(target_os = "macos")]
fn set_macos_dock_icon() {
    use cocoa::base::{id, nil};
    use cocoa::foundation::NSString;
    use objc::{msg_send, sel, sel_impl};

    unsafe {
        let app: id = msg_send![objc::class!(NSApplication), sharedApplication];
        let mut candidates = Vec::new();

        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                if let Some(contents) = exe_dir.parent() {
                    candidates.push(contents.join("Resources").join("AppIcon.icns"));
                    candidates.push(contents.join("Resources").join("assets").join("AppIcon.icns"));
                    candidates.push(contents.join("Resources").join("assets").join("favicon.png"));
                }
            }
        }
        candidates.push(std::path::PathBuf::from("assets/AppIcon.icns"));
        candidates.push(std::path::PathBuf::from("assets/favicon.png"));
        candidates.push(std::path::PathBuf::from("assets/logo.webp"));

        for path in candidates {
            if path.exists() {
                if let Ok(abs_path) = std::fs::canonicalize(&path) {
                    let path_str = abs_path.to_string_lossy();
                    let ns_str = NSString::alloc(nil).init_str(&path_str);
                    let img: id = msg_send![objc::class!(NSImage), alloc];
                    let img: id = msg_send![img, initWithContentsOfFile: ns_str];
                    if img != nil {
                        let _: () = msg_send![app, setApplicationIconImage: img];
                        info!("macOS Dock uygulama ikonu başarıyla ayarlandı: {:?}", path);
                        break;
                    }
                }
            }
        }
    }
}

fn load_tray_icon() -> Result<Icon, anyhow::Error> {
    if let Ok(img) = image::load_from_memory(include_bytes!("../assets/favicon.png")) {
        let rgba = img.into_rgba8();
        let resized = image::imageops::resize(&rgba, 32, 32, image::imageops::FilterType::Lanczos3);
        let (w, h) = (resized.width(), resized.height());
        let raw = resized.into_raw();
        if let Ok(icon) = Icon::from_rgba(raw, w, h) {
            return Ok(icon);
        }
    }

    // Dynamic 32x32 QORDY emblem icon fallback
    let width = 32;
    let height = 32;
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let dx = (x as f32) - 16.0;
            let dy = (y as f32) - 16.0;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist < 14.0 && dist > 10.0 {
                rgba.extend_from_slice(&[41, 151, 255, 255]); // blue accent ring
            } else if dist <= 10.0 && dist > 7.0 {
                rgba.extend_from_slice(&[0, 113, 227, 255]);
            } else if dist <= 4.0 {
                rgba.extend_from_slice(&[52, 199, 89, 255]); // green online dot
            } else {
                rgba.extend_from_slice(&[0, 0, 0, 0]); // transparent
            }
        }
    }
    Ok(Icon::from_rgba(rgba, width, height)?)
}

fn main() -> Result<(), anyhow::Error> {
    // 1. Loglama Başlatma
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "qordy_bridge_rs=info,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!("QORDY Termal Donanım Köprüsü (Apple Native Edition v2.5.0) başlatılıyor...");

    // 2. Yapılandırma Yöneticisi
    let config_path = std::env::var("QORDY_CONFIG_PATH").unwrap_or_else(|_| {
        // 1. Check local working directory config.json (for dev / portable mode)
        let local_cfg = std::path::PathBuf::from("config.json");
        if local_cfg.exists() && local_cfg.is_file() {
            info!("Yerel çalışma dizini yapılandırması bağlandı: {:?}", local_cfg);
            return "config.json".to_string();
        }

        // 2. Check next to executable
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                let exe_cfg = exe_dir.join("config.json");
                if exe_cfg.exists() && exe_cfg.is_file() {
                    info!("Yürütülebilir dosya yanı yapılandırması bağlandı: {:?}", exe_cfg);
                    return exe_cfg.to_string_lossy().to_string();
                }
            }
        }

        // 3. Persistent user directory (~/.qordy_bridge/config.json)
        if let Ok(home) = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
            let user_dir = std::path::PathBuf::from(home).join(".qordy_bridge");
            let user_cfg = user_dir.join("config.json");
            if user_cfg.exists() && user_cfg.is_file() {
                info!("Kullanıcı profil yapılandırması bağlandı: {:?}", user_cfg);
                return user_cfg.to_string_lossy().to_string();
            }

            // If doesn't exist yet, automatically initialize it from default template
            if let Ok(_) = std::fs::create_dir_all(&user_dir) {
                let default_content = if let Ok(exe_path) = std::env::current_exe() {
                    exe_path.parent()
                        .and_then(|p| p.parent())
                        .map(|c| c.join("Resources").join("config.default.json"))
                        .and_then(|p| std::fs::read_to_string(p).ok())
                } else {
                    None
                };

                let content_to_write = default_content.unwrap_or_else(|| {
                    include_str!("../config.json").to_string()
                });

                if let Ok(_) = std::fs::write(&user_cfg, content_to_write) {
                    info!("Varsayılan yapılandırma kullanıcı dizinine oluşturuldu: {:?}", user_cfg);
                    return user_cfg.to_string_lossy().to_string();
                }
            }
        }

        "config.json".to_string()
    });
    let config_mgr = Arc::new(ConfigManager::new(&config_path));
    let current_cfg = config_mgr.get();
    let local_port = current_cfg.local_port;

    // 3. Yazıcı Yöneticisi ve Tanımlı Yazıcıların Kaydı
    let printer_mgr = Arc::new(PrinterManager::new());

    for p in &current_cfg.printers {
        match p.printer_type.as_str() {
            "network" => {
                if let (Some(ip), Some(port)) = (&p.ip_address, p.port) {
                    match NetworkTcpPrinter::new(p.id.clone(), p.name.clone(), ip, port) {
                        Ok(net_printer) => printer_mgr.register(Arc::new(net_printer)),
                        Err(e) => error!("Ağ yazıcısı kaydedilemedi {}: {}", p.name, e),
                    }
                }
            }
            "usb" => {
                let spooler = OsSpoolerPrinter::new(p.id.clone(), p.name.clone());
                printer_mgr.register(Arc::new(spooler));
            }
            "mock" | _ => {
                let mock = MockPrinter::new(p.id.clone(), p.name.clone());
                printer_mgr.register(Arc::new(mock));
            }
        }
    }

    // 4. Bulut İstemcisi
    let cloud_client = Arc::new(CloudApiClient::new(config_mgr.clone()));

    // 5. Ana Köprü Motoru (Engine)
    let (engine, _ws_rx) = BridgeEngine::new(config_mgr.clone(), printer_mgr.clone(), cloud_client);

    // 6. Arka Plan Tokio Motorunu Başlat (Cloud Sync, Poller & Local API Server)
    let engine_srv = engine.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Tokio runtime oluşturulamadı");

        rt.block_on(async move {
            spawn_cloud_workers(engine_srv.clone());

            if let Err(e) = LocalServer::run(engine_srv, local_port).await {
                error!("Yerel sunucu hatası: {}", e);
            }
        });
    });

    // Küçük bir bekleme (Sunucunun portu dinlemeye başlaması için)
    std::thread::sleep(std::time::Duration::from_millis(350));

    // 7. Tao Native Pencere ve Event Loop
    let event_loop = EventLoop::new();

    let window_title = format!(
        "QORDY Donanım Köprüsü - {} ({})",
        current_cfg.business_name, current_cfg.branch_name
    );

    let window_icon = {
        if let Ok(img) = image::load_from_memory(include_bytes!("../assets/favicon.png")) {
            let rgba = img.into_rgba8();
            let (w, h) = rgba.dimensions();
            tao::window::Icon::from_rgba(rgba.into_raw(), w, h).ok()
        } else {
            None
        }
    };

    let mut win_builder = WindowBuilder::new()
        .with_title(&window_title)
        .with_inner_size(LogicalSize::new(1120.0, 780.0))
        .with_min_inner_size(LogicalSize::new(880.0, 580.0));

    if let Some(w_icon) = window_icon {
        win_builder = win_builder.with_window_icon(Some(w_icon));
    }

    let window = win_builder.build(&event_loop)?;

    #[cfg(target_os = "macos")]
    set_macos_dock_icon();

    // 8. Sistem Tepsisi (Tray Icon & Menu)
    let tray_menu = Menu::new();
    let title_item = MenuItem::new(
        format!("QORDY • {} (#{})", current_cfg.business_name, current_cfg.branch_code),
        false,
        None,
    );
    let sep1 = PredefinedMenuItem::separator();
    let show_item = MenuItem::new("Pencereyi Göster", true, None);
    let hide_item = MenuItem::new("Pencereyi Gizle (Arka Planda Çalış)", true, None);
    let sep2 = PredefinedMenuItem::separator();
    let test_item = MenuItem::new("Test Fişi Yazdır", true, None);
    let sep3 = PredefinedMenuItem::separator();
    let quit_item = MenuItem::new("Köprüyü Kapat ve Çık", true, None);

    let show_id = show_item.id().clone();
    let hide_id = hide_item.id().clone();
    let test_id = test_item.id().clone();
    let quit_id = quit_item.id().clone();

    let _ = tray_menu.append(&title_item);
    let _ = tray_menu.append(&sep1);
    let _ = tray_menu.append(&show_item);
    let _ = tray_menu.append(&hide_item);
    let _ = tray_menu.append(&sep2);
    let _ = tray_menu.append(&test_item);
    let _ = tray_menu.append(&sep3);
    let _ = tray_menu.append(&quit_item);

    let tray_icon = load_tray_icon().ok();
    let mut tray_builder = TrayIconBuilder::new()
        .with_menu(Box::new(tray_menu))
        .with_tooltip("QORDY Printer Bridge - Cadde Cafe");

    if let Some(icon) = tray_icon {
        tray_builder = tray_builder.with_icon(icon);
    }
    let _tray = tray_builder.build().ok();

    // 9. Wry WebKit Native Dashboard (Apple HIG UI)
    let dashboard_url = format!("http://127.0.0.1:{}", local_port);
    info!("Masaüstü penceresi oluşturuluyor: {}", dashboard_url);

    let _webview = WebViewBuilder::new()
        .with_url(&dashboard_url)
        .build(&window)?;

    info!("QORDY Apple Native Masaüstü Arayüzü başarıyla başlatıldı!");
    info!("Kapatma tuşuna (X) basıldığında uygulama arka planda çalışmaya devam eder.");

    // Event Loop ile Close Requested olayını yakala:
    // X tuşuna basıldığında pencere gizlenir, UYGULAMA ASLA KAPANMAZ!
    let menu_channel = MenuEvent::receiver();
    let tray_channel = TrayIconEvent::receiver();
    let engine_tray = engine.clone();

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        // Tray menü olaylarını kontrol et
        if let Ok(menu_event) = menu_channel.try_recv() {
            if menu_event.id == show_id {
                window.set_visible(true);
                window.set_focus();
            } else if menu_event.id == hide_id {
                window.set_visible(false);
            } else if menu_event.id == test_id {
                let eng = engine_tray.clone();
                tokio::spawn(async move {
                    let _ = eng.trigger_test_print().await;
                });
            } else if menu_event.id == quit_id {
                info!("Kullanıcı menüden çıkış yaptı. Köprü kapatılıyor...");
                *control_flow = ControlFlow::Exit;
            }
        }

        // Tepsi ikonuna sol tıklama olayını yakala (Tıklayınca pencereyi aç/odakla)
        if let Ok(tray_evt) = tray_channel.try_recv() {
            if let TrayIconEvent::Click { .. } = tray_evt {
                let is_vis = window.is_visible();
                window.set_visible(!is_vis);
                if !is_vis {
                    window.set_focus();
                }
            }
        }

        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                // KULLANICI X TUŞUNA BASTIĞINDA:
                // Pencereyi kapatma! Sadece gizle ve arka planda çalışmaya devam et!
                info!("Pencere kapatma isteği alındı. Pencere sistem tepsisine gizlendi, arka plan yazdırma aktif!");
                window.set_visible(false);
            }
            _ => (),
        }
    });
}
