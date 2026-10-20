use crate::engine::BridgeEngine;
use crate::models::{BridgeStatus, LocalPrintRequest, WsEvent};
use crate::printer::PrinterBackend;
use axum::{
    body::Body,
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Request, State,
    },
    http::{header, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::{Any, AllowOrigin, CorsLayer};
use tracing::info;

async fn auth_middleware(
    State(engine): State<Arc<BridgeEngine>>,
    request: Request<Body>,
    next: Next,
) -> Response {
    // Only enforce auth on /api/* routes; allow static assets, dashboard, and WS through.
    let path = request.uri().path();
    if !path.starts_with("/api/") && path != "/api" {
        return next.run(request).await;
    }

    let cfg = engine.config.get();

    // If no API key is configured yet (bridge not registered), allow access
    // so the user can complete registration via the dashboard.
    if !cfg.configured || cfg.api_key.is_empty() {
        return next.run(request).await;
    }

    let auth_header = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let token = auth_header.strip_prefix("Bearer ").unwrap_or("");

    if token.is_empty() || token != cfg.api_key {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "success": false, "error": "Geçersiz veya eksik yetkilendirme token'ı" })),
        )
            .into_response();
    }

    next.run(request).await
}

pub struct LocalServer;

impl LocalServer {
    pub async fn run(engine: Arc<BridgeEngine>, port: u16) -> Result<(), anyhow::Error> {
        let cors = CorsLayer::new()
            .allow_origin(AllowOrigin::predicate(|origin, _parts| {
                let origin_str = origin.to_str().unwrap_or("");
                origin_str == "http://localhost:8089"
                    || origin_str == "http://127.0.0.1:8089"
                    || origin_str.starts_with("http://localhost:")
                    || origin_str.starts_with("http://127.0.0.1:")
                    || origin_str == "null"
            }))
            .allow_methods(Any)
            .allow_headers(Any);

        let app = Router::new()
            .route("/", get(index_handler))
            .route("/assets/favicon.png", get(favicon_handler))
            .route("/assets/logo.webp", get(qordy_logo_handler))
            .route("/assets/pofuduk-logo.svg", get(pofuduk_logo_handler))
            .route("/api/status", get(status_handler))
            .route("/api/config", get(get_config_handler).post(save_config_handler))
            .route("/api/printers", get(printers_handler).post(add_printer_handler))
            .route("/api/printers/delete", post(delete_printer_handler))
            .route("/api/printers/autodiscover", post(autodiscover_printers_handler))
            .route("/api/screens", get(screens_handler))
            .route("/api/screens/assign", post(assign_screen_handler))
            .route("/api/print", post(print_handler))
            .route("/api/test-print", post(test_print_handler))
            .route("/api/diagnostic-print", post(diagnostic_print_handler))
            .route("/api/label-print", post(label_print_handler))
            .route("/api/register", post(register_handler))
            .route("/api/settings/autostart", post(set_autostart_handler))
            .route("/ws", get(ws_handler))
            .layer(middleware::from_fn_with_state(
                engine.clone(),
                auth_middleware,
            ))
            .layer(cors)
            .with_state(engine.clone());

        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let listener = tokio::net::TcpListener::bind(addr).await?;

        let local_ip = engine.local_ip.clone();

        info!("════════════════════════════════════════════════════════════");
        info!("  QORDY PRINTER BRIDGE (APPLE DESIGN NATIVE v2.5.0)");
        info!("  Yerel Masaüstü URL: http://localhost:{}", port);
        info!("  Mobil API Ağ Geçidi: http://{}:{}/api/print", local_ip, port);
        info!("════════════════════════════════════════════════════════════");

        axum::serve(listener, app).await?;
        Ok(())
    }
}

async fn status_handler(State(engine): State<Arc<BridgeEngine>>) -> Json<BridgeStatus> {
    Json(engine.get_status())
}

async fn get_config_handler(State(engine): State<Arc<BridgeEngine>>) -> impl IntoResponse {
    let cfg = engine.config.get();
    Json(serde_json::to_value(&*cfg).unwrap_or_default())
}

async fn save_config_handler(
    State(engine): State<Arc<BridgeEngine>>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    let result = engine.config.update(|cfg| {
        if let Some(name) = payload.get("business_name").and_then(|v| v.as_str()) {
            cfg.business_name = name.to_string();
        }
        if let Some(addr) = payload.get("address").and_then(|v| v.as_str()) {
            cfg.address = addr.to_string();
        }
        if let Some(phone) = payload.get("phone").and_then(|v| v.as_str()) {
            cfg.phone = phone.to_string();
        }
        if let Some(interval) = payload.get("poll_interval_secs").and_then(|v| v.as_u64()) {
            cfg.poll_interval_secs = interval;
        }
        if let Some(assignments) = payload.get("screen_assignments").and_then(|v| v.as_object()) {
            for (screen_id, printer_val) in assignments {
                if let Some(p_name) = printer_val.as_str() {
                    cfg.screen_assignments.insert(screen_id.clone(), p_name.to_string());
                }
            }
        }
    });

    match result {
        Ok(updated) => (axum::http::StatusCode::OK, Json(json!({ "success": true, "config": &*updated }))),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "success": false, "error": e.to_string() }))),
    }
}

async fn printers_handler(State(engine): State<Arc<BridgeEngine>>) -> Json<serde_json::Value> {
    let mut list = Vec::new();
    for p in engine.printers.list() {
        let is_online = p.is_online().await;
        list.push(json!({
            "id": p.id(),
            "name": p.name(),
            "type": p.printer_type(),
            "is_online": is_online
        }));
    }
    Json(json!({ "success": true, "printers": list }))
}

async fn add_printer_handler(
    State(engine): State<Arc<BridgeEngine>>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    let name = payload.get("name").and_then(|v| v.as_str()).unwrap_or("Yeni Ağ Yazıcısı");
    let p_type = payload.get("printer_type").and_then(|v| v.as_str()).unwrap_or("network");
    let ip = payload.get("ip_address").and_then(|v| v.as_str()).unwrap_or("");
    let port = payload.get("port").and_then(|v| v.as_u64()).unwrap_or(9100) as u16;
    let paper_w = payload.get("paper_width").and_then(|v| v.as_u64()).unwrap_or(48) as u8;

    let id = format!("printer-{}", &uuid::Uuid::new_v4().to_string()[..8]);

    if p_type == "mock" {
        let mock = crate::printer::MockPrinter::new(id.clone(), name.to_string());
        engine.printers.register(Arc::new(mock));
        let _ = engine.config.update(|cfg| {
            cfg.printers.push(crate::config::PrinterConfig {
                id: id.clone(),
                name: name.to_string(),
                printer_type: "mock".to_string(),
                ip_address: None,
                port: None,
                paper_width: Some(paper_w),
                is_default: false,
            });
        });
        return (axum::http::StatusCode::OK, Json(json!({ "success": true, "id": id, "message": "Sanal test yazıcısı eklendi" })));
    }

    if ip.is_empty() {
        return (axum::http::StatusCode::BAD_REQUEST, Json(json!({ "success": false, "error": "IP adresi gerekli" })));
    }

    match crate::printer::NetworkTcpPrinter::new(id.clone(), name.to_string(), ip, port) {
        Ok(printer) => {
            engine.printers.register(Arc::new(printer));
            let _ = engine.config.update(|cfg| {
                cfg.printers.push(crate::config::PrinterConfig {
                    id: id.clone(),
                    name: name.to_string(),
                    printer_type: "network".to_string(),
                    ip_address: Some(ip.to_string()),
                    port: Some(port),
                    paper_width: Some(paper_w),
                    is_default: false,
                });
            });
            (axum::http::StatusCode::OK, Json(json!({ "success": true, "id": id, "message": "Yazıcı başarıyla eklendi" })))
        }
        Err(e) => (axum::http::StatusCode::BAD_REQUEST, Json(json!({ "success": false, "error": e.to_string() }))),
    }
}

async fn delete_printer_handler(
    State(engine): State<Arc<BridgeEngine>>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    let printer_id = payload.get("id").and_then(|v| v.as_str()).unwrap_or("");
    if printer_id.is_empty() {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({ "success": false, "error": "Yazıcı ID gerekli" })),
        );
    }

    engine.printers.remove(printer_id);
    let _ = engine.config.update(|cfg| {
        cfg.printers.retain(|p| p.id != printer_id && p.name != printer_id);
    });

    (
        axum::http::StatusCode::OK,
        Json(json!({ "success": true, "message": "Yazıcı başarıyla silindi" })),
    )
}

async fn autodiscover_printers_handler(
    State(engine): State<Arc<BridgeEngine>>,
) -> impl IntoResponse {
    let discovered = crate::scanner::PrinterScanner::scan_all().await;
    let mut added_printers = Vec::new();
    let cfg = engine.config.get();

    for disc in &discovered {
        let exists = cfg.printers.iter().any(|p| {
            p.ip_address.as_deref() == Some(&disc.ip_address) && p.port == Some(disc.port)
        });

        if !exists {
            let id = format!("printer-{}", &uuid::Uuid::new_v4().to_string()[..8]);
            if disc.printer_type == "network" {
                if let Ok(net_p) = crate::printer::NetworkTcpPrinter::new(
                    id.clone(),
                    disc.name.clone(),
                    &disc.ip_address,
                    disc.port,
                ) {
                    let p_arc = Arc::new(net_p);
                    engine.printers.register(p_arc.clone());
                    let _ = engine.config.update(|c| {
                        c.printers.push(crate::config::PrinterConfig {
                            id: id.clone(),
                            name: disc.name.clone(),
                            printer_type: "network".to_string(),
                            ip_address: Some(disc.ip_address.clone()),
                            port: Some(disc.port),
                            paper_width: Some(disc.paper_width),
                            is_default: c.printers.is_empty(),
                        });
                    });

                    // Otonom ilk test çıktısı basımı — use cached generator (sync, no guard across await)
                    let test_bytes = engine.generate_test_receipt(cfg.local_port, &disc.name);
                    let _ = p_arc.print_raw(&test_bytes).await;

                    added_printers.push(disc.name.clone());
                }
            }
        }
    }

    let msg = if added_printers.is_empty() {
        format!("Tarama tamamlandı. Toplam {} cihaz yanıt verdi (Yeni cihaz eklenmedi).", discovered.len())
    } else {
        format!("{} yeni termal yazıcı otonom keşfedildi, yapılandırıldı ve test çıktısı alındı: {}", added_printers.len(), added_printers.join(", "))
    };

    (
        axum::http::StatusCode::OK,
        Json(json!({
            "success": true,
            "discovered_count": discovered.len(),
            "added_count": added_printers.len(),
            "added_printers": added_printers,
            "message": msg
        })),
    )
}

async fn screens_handler(State(engine): State<Arc<BridgeEngine>>) -> Json<serde_json::Value> {
    let cfg = engine.config.get();
    let mut screens_list = Vec::new();

    if cfg.configured && !cfg.api_key.is_empty() {
        if let Ok(cloud_screens) = engine.cloud.fetch_screens().await {
            for s in cloud_screens {
                let assigned = cfg.screen_assignments.get(&s.id).cloned().unwrap_or_default();
                screens_list.push(json!({
                    "id": s.id,
                    "name": s.name,
                    "type": s.screen_type.unwrap_or_else(|| "kitchen".to_string()),
                    "description": s.description.unwrap_or_default(),
                    "assigned": assigned,
                }));
            }
        }
    }

    if screens_list.is_empty() {
        for (screen_id, assigned_printer) in &cfg.screen_assignments {
            screens_list.push(json!({
                "id": screen_id,
                "name": screen_id,
                "description": "",
                "assigned": assigned_printer,
            }));
        }
    }

    Json(json!({ "success": true, "screens": screens_list }))
}

async fn assign_screen_handler(
    State(engine): State<Arc<BridgeEngine>>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    let screen_id = payload.get("screen_id").and_then(|v| v.as_str()).unwrap_or_default();
    let printer_id = payload.get("printer_id").and_then(|v| v.as_str()).unwrap_or_default();

    if screen_id.is_empty() {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({ "success": false, "error": "screen_id gerekli" })),
        );
    }

    let _ = engine.config.update(|cfg| {
        if printer_id.is_empty() {
            cfg.screen_assignments.remove(screen_id);
        } else {
            cfg.screen_assignments.insert(screen_id.to_string(), printer_id.to_string());
        }
    });

    (
        axum::http::StatusCode::OK,
        Json(json!({ "success": true })),
    )
}

async fn print_handler(
    State(engine): State<Arc<BridgeEngine>>,
    Json(payload): Json<LocalPrintRequest>,
) -> impl IntoResponse {
    match engine.handle_local_print(payload).await {
        Ok(resp) => (axum::http::StatusCode::OK, Json(json!(resp))),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "success": false, "error": e.to_string() })),
        ),
    }
}

async fn test_print_handler(
    State(engine): State<Arc<BridgeEngine>>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    let target_printer = payload
        .get("printer_id")
        .and_then(|v| v.as_str())
        .unwrap_or_default();

    let printer = if !target_printer.is_empty() {
        engine.printers.get(target_printer)
    } else {
        engine.printers.get_default()
    }.or_else(|| engine.printers.list().first().cloned());

    if let Some(p) = printer {
        let cfg = engine.config.get();
        let test_bytes = engine.generate_test_receipt(cfg.local_port, p.name());

        match p.print_raw(&test_bytes).await {
            Ok(_) => (
                axum::http::StatusCode::OK,
                Json(json!({ "success": true, "message": format!("{} yazıcısına test fişi gönderildi!", p.name()) })),
            ),
            Err(e) => (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "success": false, "error": format!("Test fişi basılamadı: {}", e) })),
            ),
        }
    } else {
        (
            axum::http::StatusCode::NOT_FOUND,
            Json(json!({ "success": false, "error": "Yazıcı bulunamadı" })),
        )
    }
}

async fn diagnostic_print_handler(
    State(engine): State<Arc<BridgeEngine>>,
) -> impl IntoResponse {
    let printer = engine.printers.get_default();
    if let Some(p) = printer {
        let cfg = engine.config.get();

        let status = engine.get_status();
        let active_names: Vec<String> = engine
            .printers
            .list()
            .iter()
            .map(|p| format!("{} [{}]", p.name(), p.printer_type().to_uppercase()))
            .collect();

        let diag_bytes = engine.generate_diagnostic_receipt(
            cfg.local_port,
            &active_names,
            status.uptime_seconds,
            status.jobs_printed,
        );

        match p.print_raw(&diag_bytes).await {
            Ok(_) => (
                axum::http::StatusCode::OK,
                Json(json!({ "success": true, "message": format!("{} yazıcısına sistem teşhis raporu yazdırıldı!", p.name()) })),
            ),
            Err(e) => (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "success": false, "error": format!("Teşhis raporu basılamadı: {}", e) })),
            ),
        }
    } else {
        (
            axum::http::StatusCode::NOT_FOUND,
            Json(json!({ "success": false, "error": "Sistemde kayıtlı aktif yazıcı bulunamadı" })),
        )
    }
}

async fn label_print_handler(
    State(engine): State<Arc<BridgeEngine>>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    let item_name = payload.get("item_name").and_then(|v| v.as_str()).unwrap_or("Özel Izgara Köfte");
    let table = payload.get("table_name").and_then(|v| v.as_str()).unwrap_or("Teras 1");
    let order_no = payload.get("order_no").and_then(|v| v.as_str()).unwrap_or("1042");
    let notes = payload.get("notes").and_then(|v| v.as_str()).unwrap_or("Acısız, Ekstra Yeşillik");

    let printer = engine.printers.get_default();
    if let Some(p) = printer {
        let label_bytes = engine.generate_label_receipt(item_name, table, order_no, notes);
        match p.print_raw(&label_bytes).await {
            Ok(_) => (
                axum::http::StatusCode::OK,
                Json(json!({ "success": true, "message": format!("{} yazıcısına ürün/paket etiketi basıldı!", p.name()) })),
            ),
            Err(e) => (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "success": false, "error": format!("Etiket basılamadı: {}", e) })),
            ),
        }
    } else {
        (
            axum::http::StatusCode::NOT_FOUND,
            Json(json!({ "success": false, "error": "Kayıtlı yazıcı bulunamadı" })),
        )
    }
}

async fn register_handler(
    State(engine): State<Arc<BridgeEngine>>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    let code = payload
        .get("config_code")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if code.is_empty() {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({ "success": false, "error": "Yapılandırma kodu boş olamaz" })),
        );
    }

    match engine.cloud.register(code).await {
        Ok(resp) => (axum::http::StatusCode::OK, Json(json!(resp))),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "success": false, "error": e.to_string() })),
        ),
    }
}

async fn set_autostart_handler(
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    let enable = payload.get("enable").and_then(|v| v.as_bool()).unwrap_or(true);
    match crate::autostart::AutostartManager::set_autostart(enable) {
        Ok(_) => (axum::http::StatusCode::OK, Json(json!({ "success": true, "autostart_enabled": enable }))),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "success": false, "error": e.to_string() }))),
    }
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(engine): State<Arc<BridgeEngine>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(socket, engine))
}

async fn handle_ws(mut socket: WebSocket, engine: Arc<BridgeEngine>) {
    let mut rx = engine.ws_sender.subscribe();

    let initial_status = engine.get_status();
    let _ = socket
        .send(Message::Text(
            serde_json::to_string(&WsEvent::StatusUpdate(initial_status)).unwrap_or_default(),
        ))
        .await;

    while let Ok(event) = rx.recv().await {
        if let Ok(msg) = serde_json::to_string(&event) {
            if socket.send(Message::Text(msg)).await.is_err() {
                break;
            }
        }
    }
}

async fn favicon_handler() -> impl IntoResponse {
    ([(axum::http::header::CONTENT_TYPE, "image/png")], include_bytes!("../assets/favicon.png").as_slice())
}

async fn qordy_logo_handler() -> impl IntoResponse {
    ([(axum::http::header::CONTENT_TYPE, "image/webp")], include_bytes!("../assets/logo.webp").as_slice())
}

async fn pofuduk_logo_handler() -> impl IntoResponse {
    ([(axum::http::header::CONTENT_TYPE, "image/svg+xml")], include_str!("../assets/pofuduk-logo.svg"))
}

async fn index_handler() -> Html<&'static str> {
    Html(APPLE_HIG_DASHBOARD_HTML)
}

const APPLE_HIG_DASHBOARD_HTML: &str = r###"<!DOCTYPE html>
<html lang="tr">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>QORDY Printer Bridge</title>
  <link rel="icon" type="image/png" href="/assets/favicon.png">
  <style>
    :root {
      --font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", "SF Pro Display", "Helvetica Neue", sans-serif;
      --bg-window: #fbfbfd;
      --bg-subtle: #f5f5f7;
      --bg-card: #ffffff;
      --bg-input: #f7f7f9;
      --sidebar-bg: rgba(246, 246, 248, 0.88);
      --sidebar-border: rgba(0, 0, 0, 0.07);
      
      --text-primary: #1d1d1f;
      --text-secondary: #86868b;
      --text-tertiary: #a1a1a6;
      
      --accent: #0071e3;
      --accent-hover: #0077ed;
      --accent-soft: rgba(0, 113, 227, 0.1);
      
      --green: #34c759;
      --green-soft: rgba(52, 199, 89, 0.12);
      --orange: #ff9500;
      --orange-soft: rgba(255, 149, 0, 0.12);
      --red: #ff3b30;
      --red-soft: rgba(255, 59, 48, 0.12);
      --purple: #5856d6;
      --purple-soft: rgba(88, 86, 214, 0.12);
      
      --card-border: rgba(0, 0, 0, 0.06);
      --shadow-ambient: 0 2px 14px rgba(0, 0, 0, 0.035), 0 1px 3px rgba(0, 0, 0, 0.02);
      --shadow-modal: 0 20px 50px rgba(0, 0, 0, 0.16), 0 4px 12px rgba(0, 0, 0, 0.06);
      
      --radius-sm: 8px;
      --radius-md: 12px;
      --radius-lg: 16px;
      --radius-pill: 9999px;
    }

    body.dark-theme {
      --bg-window: #141416;
      --bg-subtle: #1c1c1e;
      --bg-card: #202023;
      --bg-input: #28282b;
      --sidebar-bg: rgba(24, 24, 27, 0.88);
      --sidebar-border: rgba(255, 255, 255, 0.08);
      
      --text-primary: #f5f5f7;
      --text-secondary: #98989d;
      --text-tertiary: #636366;
      
      --accent: #2997ff;
      --accent-hover: #0077ed;
      --accent-soft: rgba(41, 151, 255, 0.15);
      
      --green: #30d158;
      --green-soft: rgba(48, 209, 88, 0.16);
      --orange: #ffd60a;
      --orange-soft: rgba(255, 214, 10, 0.16);
      --red: #ff453a;
      --red-soft: rgba(255, 69, 58, 0.16);
      --purple: #bf5af2;
      --purple-soft: rgba(191, 90, 242, 0.16);
      
      --card-border: rgba(255, 255, 255, 0.08);
      --shadow-ambient: 0 4px 20px rgba(0, 0, 0, 0.4);
      --shadow-modal: 0 24px 60px rgba(0, 0, 0, 0.7);
    }

    * {
      margin: 0;
      padding: 0;
      box-sizing: border-box;
      font-family: var(--font-family);
      -webkit-font-smoothing: antialiased;
    }

    body {
      background-color: var(--bg-window);
      color: var(--text-primary);
      height: 100vh;
      display: flex;
      overflow: hidden;
      user-select: none;
      transition: background-color 0.25s ease, color 0.25s ease;
    }

    /* macOS Window Sidebar */
    .sidebar {
      width: 250px;
      min-width: 250px;
      background: var(--sidebar-bg);
      backdrop-filter: blur(30px) saturate(190%);
      -webkit-backdrop-filter: blur(30px) saturate(190%);
      border-right: 1px solid var(--sidebar-border);
      display: flex;
      flex-direction: column;
      padding: 24px 14px 16px 14px;
    }

    .window-traffic-lights-spacer {
      height: 22px;
      margin-bottom: 12px;
    }

    .brand-header {
      display: flex;
      align-items: center;
      gap: 12px;
      padding: 0 4px 16px 4px;
      border-bottom: 1px solid var(--sidebar-border);
      margin-bottom: 16px;
    }

    .tenant-logo-wrap {
      position: relative;
      width: 44px;
      height: 44px;
      flex-shrink: 0;
    }

    .tenant-logo-img {
      width: 44px;
      height: 44px;
      border-radius: 11px;
      object-fit: cover;
      border: 1px solid rgba(0, 0, 0, 0.08);
      box-shadow: 0 2px 8px rgba(0, 0, 0, 0.08);
      display: block;
      background: var(--bg-card);
    }

    body.dark-theme .tenant-logo-img {
      border-color: rgba(255, 255, 255, 0.14);
      box-shadow: 0 3px 12px rgba(0, 0, 0, 0.35);
    }

    .brand-meta {
      min-width: 0;
      flex: 1;
      display: flex;
      flex-direction: column;
    }

    .brand-meta h1 {
      font-size: 14.5px;
      font-weight: 700;
      letter-spacing: -0.2px;
      line-height: 1.25;
      color: var(--text-primary);
      white-space: nowrap;
      overflow: hidden;
      text-overflow: ellipsis;
      margin: 0;
    }

    .tenant-branch-tag {
      font-size: 12px;
      font-weight: 600;
      color: var(--accent);
      margin-top: 2px;
      white-space: nowrap;
      overflow: hidden;
      text-overflow: ellipsis;
    }

    .tenant-code-tag {
      font-size: 11px;
      color: var(--text-secondary);
      font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
      margin-top: 1px;
      letter-spacing: 0.2px;
      white-space: nowrap;
      overflow: hidden;
      text-overflow: ellipsis;
    }

    .tenant-code-badge {
      display: inline-block;
      font-weight: 600;
      color: var(--text-primary);
    }

    .nav-section-title {
      font-size: 11px;
      font-weight: 600;
      color: var(--text-tertiary);
      text-transform: uppercase;
      letter-spacing: 0.6px;
      padding: 6px 10px 4px 10px;
    }

    .nav-list {
      list-style: none;
      display: flex;
      flex-direction: column;
      gap: 3px;
      flex: 1;
    }

    .nav-item {
      display: flex;
      align-items: center;
      gap: 11px;
      padding: 8px 12px;
      border-radius: var(--radius-sm);
      font-size: 13px;
      font-weight: 500;
      color: var(--text-primary);
      cursor: pointer;
      transition: all 0.15s ease;
    }

    .nav-item svg {
      width: 17px;
      height: 17px;
      stroke: var(--text-secondary);
      stroke-width: 1.8;
      transition: stroke 0.15s ease;
    }

    .nav-item:hover {
      background: rgba(0, 0, 0, 0.04);
    }

    body.dark-theme .nav-item:hover {
      background: rgba(255, 255, 255, 0.05);
    }

    .nav-item.active {
      background: var(--accent);
      color: #ffffff;
      font-weight: 600;
      box-shadow: 0 2px 8px var(--accent-soft);
    }

    .nav-item.active svg {
      stroke: #ffffff;
    }

    .sidebar-footer {
      border-top: 1px solid var(--sidebar-border);
      padding-top: 14px;
      display: flex;
      flex-direction: column;
      gap: 8px;
    }

    .status-pill {
      display: inline-flex;
      align-items: center;
      gap: 7px;
      padding: 5px 10px;
      border-radius: var(--radius-pill);
      background: var(--green-soft);
      color: var(--green);
      font-size: 11.5px;
      font-weight: 600;
      width: fit-content;
    }

    .status-pulse {
      width: 7px;
      height: 7px;
      background: currentColor;
      border-radius: 50%;
      box-shadow: 0 0 0 2px rgba(52, 199, 89, 0.2);
    }

    .sidebar-sys-info {
      font-size: 11px;
      color: var(--text-secondary);
      line-height: 1.5;
    }

    .theme-toggle-row {
      display: flex;
      align-items: center;
      justify-content: space-between;
      margin-top: 6px;
      padding-top: 8px;
      border-top: 1px solid var(--sidebar-border);
      font-size: 12px;
      color: var(--text-secondary);
    }

    .sidebar-brand-footer {
      margin-top: 14px;
      padding-top: 10px;
      border-top: 1px solid var(--sidebar-border);
      display: flex;
      flex-direction: column;
      gap: 7px;
    }

    .brand-crests-row {
      display: flex;
      align-items: center;
      gap: 10px;
    }

    .brand-crest-item {
      display: flex;
      align-items: center;
      gap: 6px;
    }

    .brand-crest-name {
      font-size: 11px;
      font-weight: 700;
      letter-spacing: 0.3px;
      color: var(--text-primary);
    }

    .brand-divider {
      width: 1px;
      height: 14px;
      background: var(--sidebar-border);
    }

    .brand-legal-text {
      font-size: 10px;
      color: var(--text-tertiary);
      line-height: 1.35;
    }

    /* Main Content Area */
    .main-canvas {
      flex: 1;
      display: flex;
      flex-direction: column;
      overflow-y: auto;
      padding: 34px 44px;
      background: var(--bg-window);
    }

    .tab-view {
      display: none;
      max-width: 960px;
      width: 100%;
      margin: 0 auto;
      animation: appleFadeIn 0.22s ease-out;
    }

    .tab-view.active {
      display: block;
    }

    @keyframes appleFadeIn {
      from { opacity: 0; transform: translateY(6px); }
      to { opacity: 1; transform: translateY(0); }
    }

    .view-header {
      margin-bottom: 26px;
    }

    .view-title {
      font-size: 26px;
      font-weight: 700;
      letter-spacing: -0.5px;
      margin-bottom: 4px;
      color: var(--text-primary);
    }

    .view-subtitle {
      font-size: 13.5px;
      color: var(--text-secondary);
      font-weight: 400;
    }

    /* Stat Cards Grid */
    .stats-grid {
      display: grid;
      grid-template-columns: repeat(4, 1fr);
      gap: 16px;
      margin-bottom: 26px;
    }

    .stat-card {
      background: var(--bg-card);
      border: 1px solid var(--card-border);
      border-radius: var(--radius-lg);
      padding: 18px;
      box-shadow: var(--shadow-ambient);
      display: flex;
      flex-direction: column;
      gap: 10px;
      transition: transform 0.15s ease;
    }

    .stat-card:hover {
      transform: translateY(-2px);
    }

    .stat-card-top {
      display: flex;
      align-items: center;
      justify-content: space-between;
    }

    .stat-icon-wrapper {
      width: 32px;
      height: 32px;
      border-radius: 8px;
      display: flex;
      align-items: center;
      justify-content: center;
    }

    .stat-icon-wrapper svg {
      width: 18px;
      height: 18px;
      stroke-width: 2;
    }

    .stat-label {
      font-size: 12px;
      font-weight: 500;
      color: var(--text-secondary);
    }

    .stat-value {
      font-size: 28px;
      font-weight: 700;
      letter-spacing: -0.8px;
      color: var(--text-primary);
      line-height: 1;
    }

    /* Apple Cards */
    .apple-card {
      background: var(--bg-card);
      border: 1px solid var(--card-border);
      border-radius: var(--radius-lg);
      padding: 24px;
      box-shadow: var(--shadow-ambient);
      margin-bottom: 22px;
    }

    .card-headline {
      display: flex;
      justify-content: space-between;
      align-items: center;
      margin-bottom: 16px;
    }

    .card-headline h3 {
      font-size: 16px;
      font-weight: 600;
      letter-spacing: -0.2px;
      color: var(--text-primary);
    }

    .card-headline p {
      font-size: 13px;
      color: var(--text-secondary);
    }

    /* Controls & Inputs */
    .field-group {
      margin-bottom: 16px;
    }

    .field-label {
      display: block;
      font-size: 12px;
      font-weight: 600;
      color: var(--text-secondary);
      margin-bottom: 6px;
      letter-spacing: 0.2px;
    }

    .apple-input {
      width: 100%;
      background: var(--bg-input);
      border: 1px solid var(--card-border);
      border-radius: var(--radius-sm);
      padding: 10px 14px;
      font-size: 13.5px;
      color: var(--text-primary);
      outline: none;
      transition: all 0.15s ease;
    }

    .apple-input:focus {
      background: var(--bg-card);
      border-color: var(--accent);
      box-shadow: 0 0 0 3px var(--accent-soft);
    }

    .btn-apple {
      display: inline-flex;
      align-items: center;
      justify-content: center;
      gap: 7px;
      background: var(--accent);
      color: #ffffff;
      padding: 9px 18px;
      border-radius: var(--radius-sm);
      font-size: 13px;
      font-weight: 600;
      border: none;
      cursor: pointer;
      transition: all 0.15s ease;
    }

    .btn-apple:hover {
      background: var(--accent-hover);
    }

    .btn-apple:active {
      transform: scale(0.98);
    }

    .btn-secondary {
      background: var(--bg-subtle);
      color: var(--text-primary);
      border: 1px solid var(--card-border);
    }

    .btn-secondary:hover {
      background: rgba(0, 0, 0, 0.06);
    }

    body.dark-theme .btn-secondary:hover {
      background: rgba(255, 255, 255, 0.08);
    }

    .btn-danger {
      background: var(--red-soft);
      color: var(--red);
      border: 1px solid rgba(255, 59, 48, 0.2);
    }

    .btn-danger:hover {
      background: var(--red);
      color: #ffffff;
    }

    /* List Rows */
    .apple-row {
      display: flex;
      justify-content: space-between;
      align-items: center;
      padding: 14px 0;
      border-bottom: 1px solid var(--card-border);
    }

    .apple-row:last-child {
      border-bottom: none;
    }

    .apple-row-info h4 {
      font-size: 14px;
      font-weight: 600;
      color: var(--text-primary);
      margin-bottom: 2px;
    }

    .apple-row-info p {
      font-size: 12px;
      color: var(--text-secondary);
    }

    /* Apple Switch */
    .apple-switch {
      position: relative;
      display: inline-block;
      width: 46px;
      height: 28px;
    }

    .apple-switch input {
      opacity: 0;
      width: 0;
      height: 0;
    }

    .slider {
      position: absolute;
      cursor: pointer;
      top: 0; left: 0; right: 0; bottom: 0;
      background-color: var(--bg-subtle);
      border: 1px solid var(--card-border);
      transition: .22s cubic-bezier(0.16, 1, 0.3, 1);
      border-radius: var(--radius-pill);
    }

    .slider:before {
      position: absolute;
      content: "";
      height: 22px;
      width: 22px;
      left: 2px;
      bottom: 2px;
      background-color: white;
      transition: .22s cubic-bezier(0.16, 1, 0.3, 1);
      border-radius: 50%;
      box-shadow: 0 2px 4px rgba(0,0,0,0.2);
    }

    input:checked + .slider {
      background-color: var(--green);
      border-color: var(--green);
    }

    input:checked + .slider:before {
      transform: translateX(18px);
    }

    /* Thermal Receipt Simulation Paper */
    .receipt-container {
      perspective: 1000px;
      display: flex;
      justify-content: center;
    }

    .physical-receipt {
      background: #fafaf9;
      color: #111111;
      font-family: "Courier Prime", "SF Mono", Menlo, Courier, monospace;
      padding: 24px 20px;
      width: 310px;
      box-shadow: 0 10px 30px rgba(0, 0, 0, 0.08), 0 2px 8px rgba(0, 0, 0, 0.04);
      border-radius: 4px;
      font-size: 12px;
      line-height: 1.42;
      white-space: pre-wrap;
      position: relative;
      border-top: 4px dashed #e2e2e0;
      border-bottom: 4px dashed #e2e2e0;
      transition: width 0.2s ease;
    }

    .physical-receipt.width-58 {
      width: 240px;
      font-size: 11px;
    }

    /* Terminal Console */
    .apple-terminal {
      background: #111113;
      color: #38d363;
      font-family: "SF Mono", Menlo, Monaco, Consolas, monospace;
      font-size: 12px;
      line-height: 1.6;
      padding: 16px;
      border-radius: var(--radius-md);
      height: 420px;
      overflow-y: auto;
      border: 1px solid rgba(255, 255, 255, 0.08);
      box-shadow: inset 0 2px 6px rgba(0,0,0,0.5);
    }

    /* Modal Backdrop */
    .modal-overlay {
      position: fixed;
      top: 0; left: 0; right: 0; bottom: 0;
      background: rgba(0, 0, 0, 0.4);
      backdrop-filter: blur(12px);
      -webkit-backdrop-filter: blur(12px);
      display: none;
      align-items: center;
      justify-content: center;
      z-index: 1000;
      animation: modalFadeIn 0.2s ease-out;
    }

    .modal-dialog {
      background: var(--bg-card);
      border: 1px solid var(--card-border);
      border-radius: var(--radius-lg);
      padding: 26px;
      width: 440px;
      box-shadow: var(--shadow-modal);
    }

    @keyframes modalFadeIn {
      from { opacity: 0; transform: scale(0.97); }
      to { opacity: 1; transform: scale(1); }
    }

    /* Toast Notification HUD */
    .apple-toast {
      position: fixed;
      top: 20px;
      left: 50%;
      transform: translateX(-50%) translateY(-60px);
      background: rgba(29, 29, 31, 0.92);
      color: white;
      padding: 10px 20px;
      border-radius: var(--radius-pill);
      font-size: 13px;
      font-weight: 500;
      display: flex;
      align-items: center;
      gap: 9px;
      box-shadow: 0 8px 24px rgba(0,0,0,0.2);
      backdrop-filter: blur(14px);
      transition: transform 0.3s cubic-bezier(0.16, 1, 0.3, 1), opacity 0.3s ease;
      opacity: 0;
      z-index: 2000;
      pointer-events: none;
    }

    .apple-toast.show {
      transform: translateX(-50%) translateY(0);
      opacity: 1;
    }
  </style>
</head>
<body>

  <!-- Toast HUD -->
  <div id="appleToast" class="apple-toast">
    <svg width="16" height="16" fill="none" stroke="currentColor" stroke-width="2.5" viewBox="0 0 24 24"><polyline points="20 6 9 17 4 12"></polyline></svg>
    <span id="toastMsg">İşlem başarıyla tamamlandı</span>
  </div>

  <!-- Modal: Add Printer -->
  <div id="addPrinterModal" class="modal-overlay">
    <div class="modal-dialog">
      <h3 style="font-size: 17px; font-weight: 700; margin-bottom: 6px;">Yeni Yazıcı Ekle</h3>
      <p style="font-size: 13px; color: var(--text-secondary); margin-bottom: 18px;">
        Restoran ağındaki termal ESC/POS yazıcının IP adresini girin.
      </p>

      <div class="field-group">
        <label class="field-label">YAZICI TİPİ</label>
        <select id="modalPrinterType" class="apple-input" onchange="togglePrinterTypeFields()">
          <option value="network">Ağ Yazıcısı (TCP / IP RAW 9100)</option>
          <option value="mock">Sanal Test Yazıcısı (Fiziksel Cihaz Olmadan)</option>
        </select>
      </div>

      <div class="field-group">
        <label class="field-label">YAZICI GÖRÜNEN ADI</label>
        <input type="text" id="modalPrinterName" class="apple-input" placeholder="Örn: Mutfak Yazıcısı (Epson TM-T88)" />
      </div>

      <div id="networkFields">
        <div style="display: grid; grid-template-columns: 2fr 1fr; gap: 10px;">
          <div class="field-group">
            <label class="field-label">IP ADRESİ</label>
            <input type="text" id="modalPrinterIp" class="apple-input" placeholder="Örn: 192.168.1.200" />
          </div>
          <div class="field-group">
            <label class="field-label">PORT</label>
            <input type="number" id="modalPrinterPort" class="apple-input" value="9100" />
          </div>
        </div>
      </div>

      <div class="field-group">
        <label class="field-label">KAĞIT GENİŞLİĞİ</label>
        <select id="modalPaperWidth" class="apple-input">
          <option value="48">80 mm (Standart Restoran - 48 Karakter)</option>
          <option value="32">58 mm (Mobil / Küçük - 32 Karakter)</option>
        </select>
      </div>

      <div style="display: flex; justify-content: flex-end; gap: 10px; margin-top: 24px;">
        <button class="btn-apple btn-secondary" onclick="closeAddPrinterModal()">Vazgeç</button>
        <button class="btn-apple" onclick="confirmAddPrinter()">Yazıcıyı Ekle</button>
      </div>
    </div>
  </div>

  <!-- Sidebar -->
  <aside class="sidebar">
    <div class="window-traffic-lights-spacer"></div>

    <div class="brand-header">
      <div class="tenant-logo-wrap">
        <img id="sidebarTenantLogo" src="https://qordy.com/assets/images/business/product/caddecafe_logo.webp" alt="İşletme Logosu" class="tenant-logo-img" onerror="this.src='https://qordy.com/assets/images/business/default.png'" />
      </div>
      <div class="brand-meta">
        <h1 id="sidebarTenantName">Cadde Cafe</h1>
        <div id="sidebarTenantBranch" class="tenant-branch-tag">Ana Şube</div>
        <div class="tenant-code-tag">Şube Kodu: <span id="sidebarTenantCode" class="tenant-code-badge">#296466</span></div>
      </div>
    </div>

    <div class="nav-section-title">YÖNETİM</div>
    <ul class="nav-list">
      <li class="nav-item active" onclick="switchTab('tab-setup')">
        <svg viewBox="0 0 24 24" fill="none"><rect x="3" y="3" width="7" height="7" rx="1.5"></rect><rect x="14" y="3" width="7" height="7" rx="1.5"></rect><rect x="14" y="14" width="7" height="7" rx="1.5"></rect><rect x="3" y="14" width="7" height="7" rx="1.5"></rect></svg>
        <span>Genel Bakış & Durum</span>
      </li>
      <li class="nav-item" onclick="switchTab('tab-printers')">
        <svg viewBox="0 0 24 24" fill="none"><polyline points="6 9 6 2 18 2 18 9"></polyline><path d="M6 18H4a2 2 0 0 1-2-2v-5a2 2 0 0 1 2-2h16a2 2 0 0 1 2 2v5a2 2 0 0 1-2 2h-2"></path><rect x="6" y="14" width="12" height="8" rx="1"></rect></svg>
        <span>Yazıcı Yönetimi</span>
      </li>
      <li class="nav-item" onclick="switchTab('tab-screens')">
        <svg viewBox="0 0 24 24" fill="none"><rect x="2" y="3" width="20" height="14" rx="2"></rect><line x1="8" y1="21" x2="16" y2="21"></line><line x1="12" y1="17" x2="12" y2="21"></line></svg>
        <span>Ekran Eşleştirmeleri</span>
      </li>
      <li class="nav-item" onclick="switchTab('tab-receipt')">
        <svg viewBox="0 0 24 24" fill="none"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"></path><polyline points="14 2 14 8 20 8"></polyline><line x1="16" y1="13" x2="8" y2="13"></line><line x1="16" y1="17" x2="8" y2="17"></line><polyline points="10 9 9 9 8 9"></polyline></svg>
        <span>Fiş & Etiket Tasarımı</span>
      </li>
      <li class="nav-item" onclick="switchTab('tab-logs')">
        <svg viewBox="0 0 24 24" fill="none"><polyline points="4 17 10 11 4 5"></polyline><line x1="12" y1="19" x2="20" y2="19"></line></svg>
        <span>Sistem Konsolu</span>
      </li>
    </ul>

    <div class="sidebar-footer">
      <div class="status-pill">
        <span class="status-pulse"></span>
        <span id="sidebarStatusText">Sistem Hazır</span>
      </div>
      <div class="sidebar-sys-info">
        <div id="sidebarIpText">Yerel IP: Yükleniyor...</div>
      </div>
      <div class="theme-toggle-row">
        <span>Açılışta Başlat</span>
        <label class="apple-switch">
          <input type="checkbox" id="autostartSwitch" onchange="toggleAutostart(this.checked)">
          <span class="slider"></span>
        </label>
      </div>
      <div class="theme-toggle-row">
        <span>Koyu Tema</span>
        <label class="apple-switch">
          <input type="checkbox" id="themeSwitch" onchange="toggleDarkTheme()">
          <span class="slider"></span>
        </label>
      </div>

      <div class="sidebar-brand-footer">
        <div class="brand-crests-row">
          <div class="brand-crest-item" title="QORDY Restoran Donanım Çözümleri">
            <img src="/assets/logo.webp" alt="QORDY" style="height: 16px; max-width: 68px; object-fit: contain;" />
          </div>
          <div class="brand-divider"></div>
          <div class="brand-crest-item" title="Pofuduk Dijital Medya ve Yazılım Ltd. Şti.">
            <img src="/assets/pofuduk-logo.svg" alt="Pofuduk Dijital" style="height: 16px; max-width: 75px; object-fit: contain;" />
          </div>
        </div>
        <div class="brand-legal-text">
          Pofuduk Dijital Medya ve Yazılım Ltd. Şti.<br>QORDY Donanım Köprüsü v2.5.0
        </div>
      </div>
    </div>
  </aside>

  <!-- Main Canvas -->
  <main class="main-canvas">

    <!-- TAB 1: GENEL BAKIŞ & DURUM -->
    <div id="tab-setup" class="tab-view active">
      <div class="view-header">
        <h2 class="view-title">Genel Bakış & Durum</h2>
        <p class="view-subtitle">QORDY Bulut Hesabı, Yerel Ağ ve Termal Yazıcıların Anlık Durumu</p>
      </div>

      <!-- Stat Cards -->
      <div class="stats-grid">
        <div class="stat-card">
          <div class="stat-card-top">
            <span class="stat-label">Yazdırılan Fiş</span>
            <div class="stat-icon-wrapper" style="background: var(--green-soft); color: var(--green);">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor"><polyline points="20 6 9 17 4 12"></polyline></svg>
            </div>
          </div>
          <div class="stat-value" id="statPrinted" style="color: var(--green);">0</div>
        </div>

        <div class="stat-card">
          <div class="stat-card-top">
            <span class="stat-label">Hatalı İşlem</span>
            <div class="stat-icon-wrapper" style="background: var(--red-soft); color: var(--red);">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor"><line x1="18" y1="6" x2="6" y2="18"></line><line x1="6" y1="6" x2="18" y2="18"></line></svg>
            </div>
          </div>
          <div class="stat-value" id="statFailed" style="color: var(--red);">0</div>
        </div>

        <div class="stat-card">
          <div class="stat-card-top">
            <span class="stat-label">Aktif Yazıcı</span>
            <div class="stat-icon-wrapper" style="background: var(--accent-soft); color: var(--accent);">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor"><polyline points="6 9 6 2 18 2 18 9"></polyline><path d="M6 18H4a2 2 0 0 1-2-2v-5a2 2 0 0 1 2-2h16a2 2 0 0 1 2 2v5a2 2 0 0 1-2 2h-2"></path><rect x="6" y="14" width="12" height="8"></rect></svg>
            </div>
          </div>
          <div class="stat-value" id="statPrinters">2</div>
        </div>

        <div class="stat-card">
          <div class="stat-card-top">
            <span class="stat-label">Çalışma Süresi</span>
            <div class="stat-icon-wrapper" style="background: var(--purple-soft); color: var(--purple);">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor"><circle cx="12" cy="12" r="10"></circle><polyline points="12 6 12 12 16 14"></polyline></svg>
            </div>
          </div>
          <div class="stat-value" id="statUptime">0s</div>
        </div>
      </div>

      <!-- Cloud Sync Card -->
      <div class="apple-card">
        <div class="card-headline">
          <div>
            <h3>QORDY Web Paneli Eşleşmesi</h3>
            <p>Yönetim panelinizdeki "Yazıcı Köprüsü" sekmesinden oluşturduğunuz eşleştirme kodunu girin.</p>
          </div>
          <span class="status-pill" id="cloudStatusBadge" style="background: var(--orange-soft); color: var(--orange);">
            <span class="status-pulse" style="background: currentColor;"></span>
            <span id="cloudStatusText">Eşleşme Bekleniyor</span>
          </span>
        </div>

        <div class="field-group">
          <label class="field-label">YAPILANDIRMA KODU (CONFIG CODE)</label>
          <div style="display: flex; gap: 10px;">
            <input type="text" id="setupCodeInput" class="apple-input" placeholder="Örn: 9a7b6c5d4e3f21098..." style="font-family: monospace;" />
            <button class="btn-apple" onclick="saveConfigCode()" id="btnConnectCloud">
              <svg width="15" height="15" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path d="M18 10h-1.26A8 8 0 1 0 9 20h9a5 5 0 0 0 0-10z"></path></svg>
              <span>Buluta Bağlan</span>
            </button>
          </div>
        </div>
      </div>

      <!-- Business Identity Card -->
      <div class="apple-card">
        <div class="card-headline">
          <div>
            <h3>İşletme & Fiş Bilgileri</h3>
            <p>Fiş başlığında ve sipariş adisyonlarında görünecek restoran bilgileri.</p>
          </div>
        </div>

        <div style="display: grid; grid-template-columns: 1fr 1fr; gap: 14px;">
          <div class="field-group">
            <label class="field-label">RESTORAN / İŞLETME ADI</label>
            <input type="text" id="bizNameInput" class="apple-input" placeholder="İşletme Adı" oninput="updateReceiptLivePreview()" />
          </div>
          <div class="field-group">
            <label class="field-label">İLETİŞİM TELEFONU</label>
            <input type="text" id="bizPhoneInput" class="apple-input" placeholder="Telefon Numarası" oninput="updateReceiptLivePreview()" />
          </div>
        </div>

        <div class="field-group">
          <label class="field-label">ADRES</label>
          <input type="text" id="bizAddrInput" class="apple-input" placeholder="Adres Bilgisi" oninput="updateReceiptLivePreview()" />
        </div>

        <div style="display: flex; justify-content: flex-end;">
          <button class="btn-apple btn-secondary" onclick="saveBusinessInfo()">
            <svg width="15" height="15" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><path d="M19 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11l5 5v11a2 2 0 0 1-2 2z"></path><polyline points="17 21 17 13 7 13 7 21"></polyline><polyline points="7 3 7 8 15 8"></polyline></svg>
            <span>Bilgileri Kaydet</span>
          </button>
        </div>
      </div>
    </div>

    <!-- TAB 2: YAZICILAR -->
    <div id="tab-printers" class="tab-view">
      <div class="view-header">
        <div style="display: flex; justify-content: space-between; align-items: flex-start;">
          <div>
            <h2 class="view-title">Yazıcı Yönetimi</h2>
            <p class="view-subtitle">Restorandaki tüm termal ağ yazıcıları, otonom cihaz keşfi ve test basımları.</p>
          </div>
          <div style="display: flex; gap: 10px;">
            <button class="btn-apple" onclick="autodiscoverPrinters()" id="btnAutoScan">
              <svg width="15" height="15" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><circle cx="11" cy="11" r="8"></circle><line x1="21" y1="21" x2="16.65" y2="16.65"></line><path d="M11 8v6"></path><path d="M8 11h6"></path></svg>
              <span>Ağdaki Yazıcıları Otonom Keşfet</span>
            </button>
            <button class="btn-apple btn-secondary" onclick="openAddPrinterModal()">
              <svg width="15" height="15" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><line x1="12" y1="5" x2="12" y2="19"></line><line x1="5" y1="12" x2="19" y2="12"></line></svg>
              <span>Manuel Ekle</span>
            </button>
          </div>
        </div>
      </div>

      <div class="apple-card">
        <div class="card-headline">
          <h3>Kayıtlı Termal Yazıcılar</h3>
          <span style="font-size: 12px; color: var(--text-secondary);" id="printerCountText">2 yazıcı tanımlı</span>
        </div>
        <div id="printersListContainer">
          <!-- Dinamik içerik JS tarafından yüklenecek -->
        </div>
      </div>
    </div>

    <!-- TAB 3: EKRAN EŞLEŞTİRMELERİ -->
    <div id="tab-screens" class="tab-view">
      <div class="view-header">
        <div style="display: flex; justify-content: space-between; align-items: flex-start;">
          <div>
            <h2 class="view-title">Ekran Eşleştirmeleri</h2>
            <p class="view-subtitle">Hangi bölüm siparişinin hangi termal yazıcıya yönlendirileceğini belirleyin.</p>
          </div>
          <button class="btn-apple btn-secondary" onclick="loadScreens()">
            <svg width="14" height="14" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><polyline points="23 4 23 10 17 10"></polyline><polyline points="1 20 1 14 7 14"></polyline><path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15"></path></svg>
            <span>Ekranları Yenile</span>
          </button>
        </div>
      </div>

      <div class="apple-card">
        <div class="card-headline">
          <div>
            <h3>Bölüm & Hazırlık Ekranları</h3>
            <p>Garson veya misafir sipariş verdiğinde fiş ilgili ekrana atanmış yazıcıdan otomatik çıkar.</p>
          </div>
        </div>

        <div id="screensListContainer">
          <div style="padding: 24px; text-align: center; color: var(--text-secondary); font-size: 13px;">Ekranlar yükleniyor...</div>
        </div>
      </div>
    </div>

    <!-- TAB 4: FİŞ TASARIMI -->
    <div id="tab-receipt" class="tab-view">
      <div class="view-header">
        <h2 class="view-title">Fiş & Etiket Tasarımı (Canlı Önizleme)</h2>
        <p class="view-subtitle">Termal kağıt boyutları, beeper zili, paket barkod etiketi ve Türkçe karakter optimizasyonu.</p>
      </div>

      <div style="display: grid; grid-template-columns: 1fr 1fr; gap: 24px;">
        <!-- Left: Settings -->
        <div>
          <div class="apple-card">
            <div class="card-headline">
              <h3>Baskı & Donanım Ayarları</h3>
            </div>

            <div class="apple-row">
              <div class="apple-row-info">
                <h4>Fiş / Çıktı Şablon Tipi</h4>
                <p>Önizlenecek ve test edilecek format</p>
              </div>
              <select class="apple-input" style="width: 170px;" id="previewTypeSelect" onchange="updateReceiptLivePreview()">
                <option value="kitchen">Mutfak Sipariş Fişi</option>
                <option value="customer">Müşteri Hesabı / Adisyon</option>
                <option value="waiter">Garson Bilgi Fişi</option>
                <option value="label">Paket & Ürün Etiketi (Barkodlu)</option>
              </select>
            </div>

            <div class="apple-row">
              <div class="apple-row-info">
                <h4>Türkçe Karakter Kodlaması</h4>
                <p>Restoran termal yazıcı çip desteği</p>
              </div>
              <select class="apple-input" style="width: 170px;" id="encodingSelect">
                <option value="cp857">CP857 (Türkçe POS Standardı)</option>
                <option value="cp1254">Windows-1254 (ANSI)</option>
                <option value="utf8">Doğrudan UTF-8</option>
                <option value="ascii">Güvenli ASCII Dönüşümü</option>
              </select>
            </div>

            <div class="apple-row">
              <div class="apple-row-info">
                <h4>Kağıt Genişliği</h4>
                <p>Standart 80mm veya Kompakt 58mm</p>
              </div>
              <select class="apple-input" style="width: 170px;" id="receiptPaperWidth" onchange="updateReceiptLivePreview()">
                <option value="80">80 mm (Standart POS)</option>
                <option value="58">58 mm (Kompakt / Mobil)</option>
              </select>
            </div>

            <div class="apple-row">
              <div class="apple-row-info">
                <h4>Mutfakta Zil Çal (Beeper)</h4>
                <p>Sipariş çıktığında yazıcı 2 kez bip sesi verir</p>
              </div>
              <label class="apple-switch">
                <input type="checkbox" id="receiptBeepToggle" checked onchange="updateReceiptLivePreview()">
                <span class="slider"></span>
              </label>
            </div>

            <div class="apple-row">
              <div class="apple-row-info">
                <h4>Otomatik Kağıt Kesme</h4>
                <p>Fiş bittiğinde bıçağı otomatik tetikle</p>
              </div>
              <label class="apple-switch">
                <input type="checkbox" id="receiptCutToggle" checked>
                <span class="slider"></span>
              </label>
            </div>

            <div class="apple-row">
              <div class="apple-row-info">
                <h4>Kasa Çekmecesini Aç</h4>
                <p>Hesap basıldığında para çekmecesini fırlat</p>
              </div>
              <label class="apple-switch">
                <input type="checkbox" id="receiptDrawerToggle">
                <span class="slider"></span>
              </label>
            </div>

            <div style="display: flex; gap: 8px; margin-top: 18px; padding-top: 14px; border-top: 1px solid var(--card-border); flex-wrap: wrap;">
              <button class="btn-apple" onclick="printSampleLabel()">
                <svg width="15" height="15" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><rect x="3" y="3" width="18" height="18" rx="2"></rect><line x1="7" y1="7" x2="17" y2="7"></line><line x1="7" y1="12" x2="12" y2="12"></line></svg>
                <span>Paket Etiketi Bas</span>
              </button>
              <button class="btn-apple btn-secondary" onclick="testDiagnosticReceipt()">
                <svg width="15" height="15" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><circle cx="12" cy="12" r="10"></circle><line x1="12" y1="16" x2="12" y2="12"></line><line x1="12" y1="8" x2="12.01" y2="8"></line></svg>
                <span>Teşhis Raporu Bas</span>
              </button>
            </div>
          </div>
        </div>

        <!-- Right: Physical Thermal Receipt Simulation -->
        <div>
          <div class="apple-card" style="display: flex; flex-direction: column; align-items: center;">
            <div class="card-headline" style="width: 100%;">
              <h3>Fiziksel Fiş Görünümü</h3>
              <span style="font-size: 11px; color: var(--text-secondary);" id="receiptDimensionText">80mm • 48 Sütun</span>
            </div>

            <div class="receipt-container">
              <div class="physical-receipt" id="physicalReceipt">
                <!-- Canlı oluşturulacak -->
              </div>
            </div>

            <button class="btn-apple" style="margin-top: 16px; width: 100%; max-width: 320px;" onclick="sendTestPrint('', 'Varsayılan Yazıcı')">
              <svg width="15" height="15" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><polyline points="6 9 6 2 18 2 18 9"></polyline><path d="M6 18H4a2 2 0 0 1-2-2v-5a2 2 0 0 1 2-2h16a2 2 0 0 1 2 2v5a2 2 0 0 1-2 2h-2"></path><rect x="6" y="14" width="12" height="8"></rect></svg>
              <span>Önizlenen Fişi Yazdır (Test Çıktısı Al)</span>
            </button>
          </div>
        </div>
      </div>
    </div>

    <!-- TAB: SİSTEM LOGLARI -->
    <div id="tab-logs" class="tab-view">
      <div class="view-header">
        <div style="display: flex; justify-content: space-between; align-items: flex-start;">
          <div>
            <h2 class="view-title">Sistem & Konsol Kayıtları</h2>
            <p class="view-subtitle">WebSocket üzerinden canlı akan sıfır gecikmeli olay kayıtları.</p>
          </div>
          <div style="display: flex; gap: 8px;">
            <button class="btn-apple btn-secondary" onclick="clearLogs()">Temizle</button>
            <button class="btn-apple btn-secondary" onclick="exportLogs()">Dışa Aktar</button>
          </div>
        </div>
      </div>

      <div class="apple-card" style="padding: 16px;">
        <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 12px;">
          <input type="text" id="logSearchInput" class="apple-input" placeholder="Loglarda ara (örn: Mutfak, Sipariş, Hata)..." style="width: 320px;" oninput="filterLogs()" />
          <div style="display: flex; align-items: center; gap: 8px; font-size: 12px; color: var(--text-secondary);">
            <span>Otomatik Kaydır</span>
            <label class="apple-switch" style="width: 36px; height: 22px;">
              <input type="checkbox" id="autoScrollSwitch" checked>
              <span class="slider" style="border-radius: 22px;"></span>
            </label>
          </div>
        </div>

        <div class="apple-terminal" id="terminalBox">
          <div style="color: #636366;">[BAŞLATMA] QORDY Donanım Köprüsü v2.5.0 yüklendi.</div>
          <div style="color: #636366;">[BİLGİ] Donanım iletişim servisi devrede. Sistem hazır.</div>
        </div>
      </div>
    </div>

  </main>

  <script>
    let currentConfig = null;
    let printersList = [];
    let logLines = [];

    function showToast(msg) {
      const toast = document.getElementById('appleToast');
      document.getElementById('toastMsg').innerText = msg.replace(/^✓\s*/, '');
      toast.classList.add('show');
      setTimeout(() => toast.classList.remove('show'), 2800);
    }

    function switchTab(tabId) {
      document.querySelectorAll('.tab-view').forEach(el => el.classList.remove('active'));
      document.querySelectorAll('.nav-item').forEach(el => el.classList.remove('active'));

      const target = document.getElementById(tabId);
      if (target) target.classList.add('active');

      const tabs = ['tab-setup', 'tab-printers', 'tab-screens', 'tab-receipt', 'tab-mobile', 'tab-logs'];
      const idx = tabs.indexOf(tabId);
      if (idx !== -1) {
        document.querySelectorAll('.nav-item')[idx].classList.add('active');
      }

      if (tabId === 'tab-screens') {
        loadScreens();
      }
    }

    function toggleDarkTheme() {
      const isDark = document.getElementById('themeSwitch').checked;
      if (isDark) {
        document.body.classList.add('dark-theme');
      } else {
        document.body.classList.remove('dark-theme');
      }
    }

    // Modal Operations
    function openAddPrinterModal() {
      document.getElementById('addPrinterModal').style.display = 'flex';
      document.getElementById('modalPrinterName').focus();
    }

    function closeAddPrinterModal() {
      document.getElementById('addPrinterModal').style.display = 'none';
      document.getElementById('modalPrinterName').value = '';
      document.getElementById('modalPrinterIp').value = '';
    }

    function togglePrinterTypeFields() {
      const type = document.getElementById('modalPrinterType').value;
      document.getElementById('networkFields').style.display = type === 'network' ? 'block' : 'none';
    }

    async function autodiscoverPrinters() {
      const btn = document.getElementById('btnAutoScan');
      btn.innerHTML = '<span>Taranıyor (Subnet/USB)...</span>';
      btn.style.opacity = '0.7';
      btn.disabled = true;

      appendLog('[TARAMA] Yerel ağdaki termal yazıcılar taranıyor (Subnet RAW 9100 + USB)...');

      try {
        const res = await fetch('/api/printers/autodiscover', { method: 'POST' });
        const data = await res.json();
        if (data.success) {
          showToast(data.message);
          appendLog(`[KEŞİF TAMAMLANDI] ${data.message}`);
          loadPrinters();
        } else {
          alert('Tarama hatası: ' + data.error);
        }
      } catch (e) {
        appendLog(`[AĞ HATASI] ${e}`);
      } finally {
        btn.innerHTML = `
          <svg width="15" height="15" fill="none" stroke="currentColor" stroke-width="2" viewBox="0 0 24 24"><circle cx="11" cy="11" r="8"></circle><line x1="21" y1="21" x2="16.65" y2="16.65"></line><path d="M11 8v6"></path><path d="M8 11h6"></path></svg>
          <span>Ağdaki Yazıcıları Otonom Keşfet</span>
        `;
        btn.style.opacity = '1';
        btn.disabled = false;
      }
    }

    async function confirmAddPrinter() {
      const type = document.getElementById('modalPrinterType').value;
      const name = document.getElementById('modalPrinterName').value.trim() || 'Yeni Yazıcı';
      const ip = document.getElementById('modalPrinterIp').value.trim();
      const port = parseInt(document.getElementById('modalPrinterPort').value) || 9100;
      const width = parseInt(document.getElementById('modalPaperWidth').value) || 48;

      if (type === 'network' && !ip) {
        alert('Lütfen geçerli bir IP adresi girin');
        return;
      }

      try {
        const res = await fetch('/api/printers', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            name: name,
            printer_type: type,
            ip_address: ip,
            port: port,
            paper_width: width
          })
        });

        const data = await res.json();
        if (data.success) {
          showToast(data.message);
          appendLog(`[YAZICI EKLENDİ] ${name} (${type}) sisteme kaydedildi.`);
          closeAddPrinterModal();
          loadPrinters();
        } else {
          alert('Hata: ' + data.error);
        }
      } catch (e) {
        alert('Ağ Hatası: ' + e);
      }
    }

    async function deletePrinter(id, name) {
      if (!confirm(`"${name}" yazıcısını silmek istediğinize emin misiniz?`)) return;

      try {
        const res = await fetch('/api/printers/delete', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ id: id })
        });
        const data = await res.json();
        if (data.success) {
          showToast('Yazıcı silindi');
          appendLog(`[YAZICI SİLİNDİ] ${name} sistemden kaldırıldı.`);
          loadPrinters();
        } else {
          alert('Hata: ' + data.error);
        }
      } catch (e) {
        alert('Hata: ' + e);
      }
    }

    async function loadStatus() {
      try {
        const res = await fetch('/api/status');
        const data = await res.json();
        document.getElementById('statPrinted').innerText = data.jobs_printed;
        document.getElementById('statFailed').innerText = data.jobs_failed;
        document.getElementById('statPrinters').innerText = data.active_printers;
        document.getElementById('statUptime').innerText = data.uptime_seconds + 's';

        const ipPort = `${data.local_ip}:${data.local_port}`;
        if (document.getElementById('sidebarIpText')) {
          document.getElementById('sidebarIpText').innerText = `Yerel IP: ${ipPort}`;
        }

        if (data.is_configured && data.business_name) {
          if (document.getElementById('sidebarTenantLogo') && data.logo_url) {
            document.getElementById('sidebarTenantLogo').src = data.logo_url;
          }
          if (document.getElementById('sidebarTenantName')) {
            document.getElementById('sidebarTenantName').innerText = data.business_name;
          }
          if (document.getElementById('sidebarTenantBranch')) {
            if (data.branch_name) {
              document.getElementById('sidebarTenantBranch').innerText = data.branch_name;
              document.getElementById('sidebarTenantBranch').style.display = 'block';
            } else {
              document.getElementById('sidebarTenantBranch').style.display = 'none';
            }
          }
          if (document.getElementById('sidebarTenantCode')) {
            const rawCode = data.branch_code || data.business_number || '';
            document.getElementById('sidebarTenantCode').innerText = rawCode ? (rawCode.startsWith('#') ? rawCode : `#${rawCode}`) : '-';
          }
        } else {
          if (document.getElementById('sidebarTenantName')) {
            document.getElementById('sidebarTenantName').innerText = 'Bağlantı Yok';
          }
          if (document.getElementById('sidebarTenantBranch')) {
            document.getElementById('sidebarTenantBranch').innerText = 'Yapılandırma Bekleniyor';
          }
          if (document.getElementById('sidebarTenantCode')) {
            document.getElementById('sidebarTenantCode').innerText = '-';
          }
        }

        if (document.getElementById('sidebarStatusText')) {
          if (data.is_cloud_connected) {
            document.getElementById('sidebarStatusText').innerText = 'Bulut Senkronize';
          } else {
            document.getElementById('sidebarStatusText').innerText = 'Sistem Hazır (Yerel)';
          }
        }

        if (document.activeElement !== document.getElementById('bizNameInput') && document.getElementById('bizNameInput')) {
          document.getElementById('bizNameInput').value = data.business_name || '';
        }
        if (document.activeElement !== document.getElementById('bizPhoneInput') && document.getElementById('bizPhoneInput')) {
          document.getElementById('bizPhoneInput').value = data.phone || '';
        }
        if (document.activeElement !== document.getElementById('bizAddrInput') && document.getElementById('bizAddrInput')) {
          document.getElementById('bizAddrInput').value = data.address || '';
        }

        if (document.getElementById('autostartSwitch')) {
          document.getElementById('autostartSwitch').checked = !!data.autostart_enabled;
        }

        if (data.is_cloud_connected) {
          document.getElementById('cloudStatusBadge').style.background = 'var(--green-soft)';
          document.getElementById('cloudStatusBadge').style.color = 'var(--green)';
          document.getElementById('cloudStatusText').innerText = 'Bulut Senkronize';
        } else {
          document.getElementById('cloudStatusBadge').style.background = 'var(--orange-soft)';
          document.getElementById('cloudStatusBadge').style.color = 'var(--orange)';
          document.getElementById('cloudStatusText').innerText = 'Bağlantı Bekleniyor';
        }
      } catch (e) {
        console.error(e);
      }
    }

    async function toggleAutostart(enable) {
      try {
        const res = await fetch('/api/settings/autostart', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ enable: enable })
        });
        const data = await res.json();
        if (data.success) {
          showToast(enable ? 'Bilgisayar açılışında otomatik başlatma aktif edildi' : 'Açılışta otomatik başlatma kapatıldı');
          appendLog(`[SİSTEM] Açılışta başlatma: ${enable ? 'Aktif' : 'Pasif'}`);
        } else {
          alert('Ayar güncellenemedi: ' + data.error);
        }
      } catch (e) {
        alert('Hata: ' + e);
      }
    }

    async function loadPrinters() {
      try {
        const res = await fetch('/api/printers');
        const data = await res.json();
        printersList = data.printers || [];

        document.getElementById('printerCountText').innerText = `${printersList.length} yazıcı tanımlı`;

        const container = document.getElementById('printersListContainer');
        container.innerHTML = '';

        printersList.forEach(p => {
          const row = document.createElement('div');
          row.className = 'apple-row';
          const typeBadge = p.type === 'network' ? 'AĞ (TCP 9100)' : (p.type === 'mock' ? 'SANAL (MOCK)' : 'USB');
          const isOnline = p.is_online;

          row.innerHTML = `
            <div class="apple-row-info">
              <h4>${p.name}</h4>
              <p>Tip: <strong style="color: var(--text-primary)">${typeBadge}</strong> • Durum: <span style="color: ${isOnline ? 'var(--green)' : 'var(--orange)'}; font-weight: 600;">${isOnline ? 'Hazır' : 'Beklemede'}</span></p>
            </div>
            <div style="display: flex; gap: 8px;">
              <button class="btn-apple btn-secondary" onclick="sendTestPrint('${p.id}', '${p.name}')">Test Fişi Bas</button>
              <button class="btn-apple btn-danger" onclick="deletePrinter('${p.id}', '${p.name}')">Sil</button>
            </div>
          `;
          container.appendChild(row);
        });

        // Dropdownları doldur
        const screenSelects = ['map-kitchen', 'map-cashier', 'map-waiter', 'map-preparation', 'map-bar'];
        screenSelects.forEach(id => {
          const sel = document.getElementById(id);
          if (!sel) return;
          const currentVal = sel.value;
          sel.innerHTML = '<option value="">(Seçilmedi)</option>';
          printersList.forEach(p => {
            const opt = document.createElement('option');
            opt.value = p.id;
            opt.innerText = p.name;
            sel.appendChild(opt);
          });
          if (currentVal) sel.value = currentVal;
        });

        loadConfig();
      } catch (e) {
        console.error(e);
      }
    }

    async function loadConfig() {
      try {
        const res = await fetch('/api/config');
        currentConfig = await res.json();

        if (currentConfig.business_name && document.getElementById('bizNameInput')) document.getElementById('bizNameInput').value = currentConfig.business_name;
        if (currentConfig.address && document.getElementById('bizAddrInput')) document.getElementById('bizAddrInput').value = currentConfig.address;
        if (currentConfig.phone && document.getElementById('bizPhoneInput')) document.getElementById('bizPhoneInput').value = currentConfig.phone;
        if (currentConfig.config_code && document.getElementById('setupCodeInput')) document.getElementById('setupCodeInput').value = currentConfig.config_code;

        loadScreens();
        updateReceiptLivePreview();
      } catch (e) {
        console.error(e);
      }
    }

    async function saveBusinessInfo() {
      const name = document.getElementById('bizNameInput').value;
      const addr = document.getElementById('bizAddrInput').value;
      const phone = document.getElementById('bizPhoneInput').value;

      try {
        await fetch('/api/config', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ business_name: name, address: addr, phone: phone })
        });
        showToast('İşletme bilgileri kaydedildi');
        appendLog('[AYARLAR] İşletme bilgileri güncellendi.');
        updateReceiptLivePreview();
      } catch (e) {
        alert('Hata: ' + e);
      }
    }

    async function loadScreens() {
      try {
        const res = await fetch('/api/screens');
        const data = await res.json();
        const container = document.getElementById('screensListContainer');
        if (!container) return;

        if (!data.screens || data.screens.length === 0) {
          container.innerHTML = '<div style="padding: 24px; text-align: center; color: var(--text-secondary); font-size: 13px;">Bağlı bir işletme ekranı bulunamadı. Buluta bağlandıktan sonra hazırlık ve mutfak ekranlarınız burada listelenir.</div>';
          return;
        }

        const prRes = await fetch('/api/printers');
        const prData = await prRes.json();
        const printerList = prData.printers || [];

        let html = '';
        for (const scr of data.screens) {
          let opts = '<option value="">(Yazıcı Atanmadı)</option>';
          for (const p of printerList) {
            const isSel = (scr.assigned === p.id) ? 'selected' : '';
            opts += `<option value="${p.id}" ${isSel}>${p.name}</option>`;
          }

          html += `
            <div class="apple-row">
              <div class="apple-row-info">
                <h4>${scr.name} <span style="font-size: 11px; font-weight: normal; color: var(--text-tertiary); font-family: monospace;">(${scr.id})</span></h4>
                <p>${scr.description || 'Bu ekrana gelen siparişler için hedef termal yazıcıyı seçin'}</p>
              </div>
              <select class="apple-input" style="width: 240px;" data-screen-id="${scr.id}" onchange="saveScreenMapping(this)">
                ${opts}
              </select>
            </div>
          `;
        }
        container.innerHTML = html;
      } catch (e) {
        console.error(e);
      }
    }

    async function saveScreenMapping(selectEl) {
      const screenId = selectEl.getAttribute('data-screen-id');
      const printerId = selectEl.value;
      try {
        const res = await fetch('/api/screens/assign', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ screen_id: screenId, printer_id: printerId })
        });
        const data = await res.json();
        if (data.success) {
          showToast('Ekran-yazıcı rotası kaydedildi');
          appendLog(`[EŞLEŞTİRME] Ekran: ${screenId} -> Yazıcı: ${printerId || 'Yok'}`);
        } else {
          alert('Atama hatası: ' + data.error);
        }
      } catch (e) {
        console.error(e);
      }
    }

    async function saveConfigCode() {
      const code = document.getElementById('setupCodeInput').value.trim();
      if (!code) {
        alert('Lütfen yapılandırma kodunu girin');
        return;
      }
      appendLog(`[BULUT] Yapılandırma kodu doğrulanıyor: ${code.slice(0, 10)}...`);
      try {
        const res = await fetch('/api/register', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ config_code: code })
        });
        const data = await res.json();
        if (data.success) {
          showToast('QORDY Bulut Hesabı Başarıyla Eşleştirildi!');
          appendLog('[BULUT] Bağlantı başarılı! İşletme ve şube verileri senkronize edildi.');
          await loadConfig();
          await loadStatus();
        } else {
          alert('Bulut Hatası: ' + data.error);
        }
      } catch (e) {
        alert('Bağlantı hatası: ' + e);
      }
    }

    async function sendTestPrint(id, name) {
      appendLog(`[TEST] "${name}" yazıcısına test fişi basılıyor...`);
      try {
        const res = await fetch('/api/test-print', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ printer_id: id })
        });
        const data = await res.json();
        if (data.success) {
          showToast('Test Fişi Gönderildi');
          appendLog(`[OK] ${data.message}`);
          loadStatus();
        } else {
          alert('Baskı Hatası: ' + data.error);
        }
      } catch (e) {
        appendLog(`[HATA] ${e}`);
      }
    }

    async function testDiagnosticReceipt() {
      appendLog('[RAPOR] Sistem teşhis raporu yazdırılıyor...');
      try {
        const res = await fetch('/api/diagnostic-print', { method: 'POST' });
        const data = await res.json();
        if (data.success) {
          showToast('Sistem Teşhis Raporu Basıldı');
          appendLog(`[OK] ${data.message}`);
          loadStatus();
        } else {
          alert('Hata: ' + data.error);
        }
      } catch (e) {
        appendLog(`[HATA] ${e}`);
      }
    }

    async function printSampleLabel() {
      appendLog('[ETİKET] Paket / Ürün barkod etiketi basılıyor...');
      try {
        const res = await fetch('/api/label-print', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            item_name: "Özel Karışık Izgara",
            table_name: "Teras 1",
            order_no: "VIP-9002",
            notes: "Acısız, Ekstra Yeşillik"
          })
        });
        const data = await res.json();
        if (data.success) {
          showToast('Ürün Etiketi Basıldı');
          appendLog(`[OK] ${data.message}`);
        } else {
          alert('Hata: ' + data.error);
        }
      } catch (e) {
        appendLog(`[HATA] ${e}`);
      }
    }

    async function simulateMobileOrder() {
      appendLog('[MOBİL] Garson iPhone üzerinden sipariş basılıyor...');
      const orderId = 'ORD-' + Math.floor(1000 + Math.random() * 9000);
      const req = {
        screen_id: 'kitchen_main',
        order_id: orderId,
        table_name: 'Teras 1',
        waiter_name: 'Burak Garson',
        items: [
          { name: 'Karisik Izgara', quantity: 1, price: 450.0, notes: 'Az pismis' },
          { name: 'Kunefe', quantity: 2, price: 120.0, notes: 'Kaymakli' }
        ],
        total_amount: 690.0,
        notes: 'VIP Misafir'
      };

      try {
        const res = await fetch('/api/print', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(req)
        });
        const data = await res.json();
        if (data.success) {
          showToast(`Mobil Sipariş Basıldı (#${orderId})`);
          appendLog(`[YAZDIRILDI] #${orderId} -> ${data.target_printer} (Teras 1)`);
          loadStatus();
        } else {
          alert('Yazdırma Hatası: ' + data.error);
        }
      } catch (e) {
        appendLog(`[HATA] ${e}`);
      }
    }

    function copyMobileUrl() {
      const input = document.getElementById('mobileEndpointDisplay');
      input.select();
      navigator.clipboard.writeText(input.value);
      showToast('Mobil API URL Panoya Kopyalandı');
    }

    // Live Receipt Preview Renderer
    function updateReceiptLivePreview() {
      const type = document.getElementById('previewTypeSelect').value;
      const width = document.getElementById('receiptPaperWidth').value;
      const bizName = document.getElementById('bizNameInput').value || 'İşletme Adı';
      const phone = document.getElementById('bizPhoneInput').value || '';
      const addr = document.getElementById('bizAddrInput').value || '';
      const beeper = document.getElementById('receiptBeepToggle').checked;

      const paper = document.getElementById('physicalReceipt');
      if (width === '58') {
        paper.classList.add('width-58');
        document.getElementById('receiptDimensionText').innerText = '58mm • 32 Sütun';
      } else {
        paper.classList.remove('width-58');
        document.getElementById('receiptDimensionText').innerText = '80mm • 48 Sütun';
      }

      let content = '';
      const timeStr = new Date().toLocaleTimeString('tr-TR', { hour: '2-digit', minute: '2-digit' });
      const dateStr = new Date().toLocaleDateString('tr-TR');

      if (type === 'kitchen') {
        content = `
========================================
             MUTFAK SIPARISI
             [ SICAK MUTFAK ]
========================================
MASA: BAHCE 4
Garson: Canan G.             No: #1042
Saat: ${timeStr}               ${dateStr}
----------------------------------------
ADET   URUN
----------------------------------------
2x     Adana Kebap
       >> NOT: Acisiz olsun
1x     Karisik Izgara
       + Ekstra Koz Biber
2x     Yayik Ayrani
----------------------------------------
SIPARIS NOTU: Misafir bekletilmesin!
========================================
${beeper ? '[BEEPER ZILI TETIKLENDI]\n' : ''}[OTOMATIK KAGIT KESIMI]
        `;
      } else if (type === 'customer') {
        content = `
========================================
             ${bizName.toUpperCase()}
          ${addr}
            Tel: ${phone}
========================================
Fis No: #REC-8091          Masa: Bahce 4
Tarih: ${dateStr} ${timeStr}
----------------------------------------
URUN / MIKTAR                      TUTAR
----------------------------------------
2x Adana Kebap                 560.00 TL
1x Karisik Izgara              450.00 TL
2x Yayik Ayrani                 70.00 TL
----------------------------------------
Ara Toplam:                   1080.00 TL
KDV (%10 Dahil):                98.18 TL
========================================
TOPLAM:                       1080.00 TL
========================================
Odeme: KREDI KARTI
Tesekkur ederiz, yine bekleriz!
www.qordy.com
[OTOMATIK KAGIT KESIMI]
        `;
      } else if (type === 'label') {
        content = `
========================================
           PAKET & URUN ETIKETI
                 #VIP-1042
             MASA: BAHCE 4
----------------------------------------
2x KARISIK IZGARA
   + Az pismis
   + Kozlenmis biber
----------------------------------------
NOT: Paket servis - Kurye teslimi
||| | ||||| |||| || |||| ||||| ||| ||||
           *VIP-1042*
${dateStr} ${timeStr}
========================================
        `;
      } else {
        content = `
----------------------------------------
             BAHCE 4 - ADISYON
----------------------------------------
Garson: Canan G.
Acilis: ${timeStr}
----------------------------------------
2x Adana Kebap                 560.00 TL
1x Karisik Izgara              450.00 TL
2x Yayik Ayrani                 70.00 TL
----------------------------------------
ARA TOPLAM:                   1080.00 TL
----------------------------------------
[OTOMATIK KAGIT KESIMI]
        `;
      }

      paper.innerText = content.trim();
    }

    function appendLog(line) {
      const box = document.getElementById('terminalBox');
      const time = new Date().toLocaleTimeString('tr-TR');
      const formatted = `[${time}] ${line}`;
      logLines.push(formatted);

      const div = document.createElement('div');
      div.innerText = formatted;

      if (line.includes('[HATA]')) div.style.color = '#ff453a';
      else if (line.includes('[YAZDIRILDI]') || line.includes('[OK]')) div.style.color = '#30d158';
      else if (line.includes('[BULUT]')) div.style.color = '#2997ff';
      else if (line.includes('[MOBİL]')) div.style.color = '#bf5af2';
      else if (line.includes('[UYARI]')) div.style.color = '#ffd60a';

      box.appendChild(div);

      if (document.getElementById('autoScrollSwitch').checked) {
        box.scrollTop = box.scrollHeight;
      }
    }

    function clearLogs() {
      document.getElementById('terminalBox').innerHTML = '<div style="color: #636366;">[KONSOL SIFIRLANDI]</div>';
      logLines = [];
      showToast('Konsol temizlendi');
    }

    function filterLogs() {
      const q = document.getElementById('logSearchInput').value.toLowerCase();
      const rows = document.querySelectorAll('#terminalBox div');
      rows.forEach(r => {
        if (!q || r.innerText.toLowerCase().includes(q)) {
          r.style.display = 'block';
        } else {
          r.style.display = 'none';
        }
      });
    }

    function exportLogs() {
      const blob = new Blob([logLines.join('\n')], { type: 'text/plain' });
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = `qordy_bridge_logs_${Date.now()}.txt`;
      a.click();
      URL.revokeObjectURL(url);
    }

    function connectWs() {
      const protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
      const ws = new WebSocket(`${protocol}//${location.host}/ws`);

      ws.onmessage = (event) => {
        try {
          const msg = JSON.parse(event.data);
          if (msg.type === 'JobPrinted') {
            appendLog(`[YAZDIRILDI] Masa: ${msg.data.table_name || '-'} | #${msg.data.order_id || '-'} -> ${msg.data.printer_name}`);
            loadStatus();
          } else if (msg.type === 'JobFailed') {
            appendLog(`[HATA] Sipariş #${msg.data.order_id || '-'} basılamadı: ${msg.data.error}`);
            loadStatus();
          }
        } catch (_) {}
      };

      ws.onclose = () => setTimeout(connectWs, 3000);
    }

    // Auto-detect OS dark mode
    if (window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches) {
      document.getElementById('themeSwitch').checked = true;
      document.body.classList.add('dark-theme');
    }

    loadStatus();
    loadPrinters();
    loadScreens();
    connectWs();
    setInterval(loadStatus, 4000);
  </script>
</body>
</html>"###;
