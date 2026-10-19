#![allow(dead_code)]
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterRequest {
    pub bridge_id: String,
    pub config_code: String,
    pub device_name: String,
    pub version: String,
    pub os: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterResponse {
    pub success: bool,
    pub error: Option<String>,
    pub bridge_id: Option<String>,
    pub api_key: Option<String>,
    pub business_id: Option<String>,
    pub business_name: Option<String>,
    pub branch_name: Option<String>,
    pub branch_code: Option<String>,
    pub address: Option<String>,
    pub phone: Option<String>,
    pub logo_url: Option<String>,
    pub business_number: Option<String>,
    pub business: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatRequest {
    pub bridge_id: String,
    pub api_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatResponse {
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Screen {
    #[serde(alias = "screen_id")]
    pub id: String,
    pub name: String,
    #[serde(rename = "type", alias = "screen_type")]
    pub screen_type: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreensResponse {
    pub success: bool,
    pub screens: Option<Vec<Screen>>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderItem {
    pub name: String,
    pub quantity: f64,
    pub price: Option<f64>,
    pub notes: Option<String>,
    pub options: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrintJob {
    #[serde(alias = "id")]
    pub queue_id: Option<String>,
    pub screen_id: Option<String>,
    pub screen_name: Option<String>,
    pub screen_type: Option<String>,
    pub order_id: Option<String>,
    pub table_name: Option<String>,
    pub waiter_name: Option<String>,
    pub content: Option<String>, // ESC/POS raw string or legacy text
    pub items: Option<Vec<OrderItem>>,
    pub total_amount: Option<f64>,
    pub payment_type: Option<String>,
    pub notes: Option<String>,
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueResponse {
    pub success: bool,
    pub jobs: Option<Vec<PrintJob>>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateStatusRequest {
    pub bridge_id: String,
    pub api_key: String,
    pub queue_id: String,
    pub status: String, // "PRINTED" or "FAILED"
    pub error_message: Option<String>,
}

/// Mobil uygulamalardan (iOS / Android Flutter) gelen doğrudan yazdırma isteği
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalPrintRequest {
    pub screen_id: Option<String>,
    pub target_printer: Option<String>,
    pub order_id: Option<String>,
    pub table_name: Option<String>,
    pub waiter_name: Option<String>,
    pub items: Option<Vec<OrderItem>>,
    pub total_amount: Option<f64>,
    pub payment_type: Option<String>,
    pub notes: Option<String>,
    pub raw_text: Option<String>,
    pub raw_escpos_base64: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalPrintResponse {
    pub success: bool,
    pub job_id: String,
    pub message: String,
    pub target_printer: String,
    pub printed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeStatus {
    pub version: String,
    pub bridge_id: String,
    pub business_name: String,
    pub branch_name: String,
    pub branch_code: String,
    pub address: String,
    pub phone: String,
    pub logo_url: Option<String>,
    pub business_number: Option<String>,
    pub is_configured: bool,
    pub is_cloud_connected: bool,
    pub local_ip: String,
    pub local_port: u16,
    pub active_printers: usize,
    pub jobs_printed: u64,
    pub jobs_failed: u64,
    pub uptime_seconds: u64,
    pub autostart_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum WsEvent {
    Log {
        level: String,
        message: String,
        timestamp: String,
    },
    JobPrinted {
        order_id: String,
        table_name: String,
        printer_name: String,
        timestamp: String,
    },
    JobFailed {
        order_id: String,
        error: String,
        timestamp: String,
    },
    StatusUpdate(BridgeStatus),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_print_job_json_deserialization() {
        let json_str = r#"{
            "id": "q-1234",
            "screen_id": "screen-kitchen",
            "screen_name": "Mutfak",
            "screen_type": "kitchen",
            "order_id": "ORD-582",
            "table_name": "Masa 4",
            "waiter_name": "Ahmet",
            "items": [
                {
                    "name": "Köfte Porsiyon",
                    "quantity": 2.0,
                    "price": 240.0,
                    "notes": "Az pişmiş"
                }
            ],
            "total_amount": 480.0
        }"#;

        let job: PrintJob = serde_json::from_str(json_str).expect("Failed to deserialize PrintJob");
        assert_eq!(job.queue_id.as_deref(), Some("q-1234"));
        assert_eq!(job.order_id.as_deref(), Some("ORD-582"));
        assert_eq!(job.items.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn test_local_print_request() {
        let req = LocalPrintRequest {
            screen_id: Some("kitchen".to_string()),
            target_printer: None,
            order_id: Some("123".to_string()),
            table_name: Some("Masa 1".to_string()),
            waiter_name: Some("Mehmet".to_string()),
            items: None,
            total_amount: Some(150.0),
            payment_type: Some("Nakit".to_string()),
            notes: None,
            raw_text: None,
            raw_escpos_base64: None,
        };
        let s = serde_json::to_string(&req).expect("Failed to serialize LocalPrintRequest");
        assert!(s.contains("Masa 1"));
    }
}
