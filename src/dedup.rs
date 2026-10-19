use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Çoklu cihaz veya ardışık isteklerde çifte yazdırmayı (duplicate print)
/// ve kağıt israfını önleyen İdempotency Koruma Halkası.
pub struct DeduplicationRing {
    history: Mutex<HashMap<String, Instant>>,
    ttl: Duration,
}

impl DeduplicationRing {
    pub fn new(ttl_seconds: u64) -> Self {
        Self {
            history: Mutex::new(HashMap::new()),
            ttl: Duration::from_secs(ttl_seconds),
        }
    }

    /// Eğer bu sipariş/fiş son `ttl` süresi içinde basıldıysa `false` (çifte basım, engelle) döner.
    /// İlk kez geliyorsa kaydeder ve `true` (basıma izin ver) döner.
    pub fn check_and_record(&self, fingerprint: &str) -> bool {
        let mut map = match self.history.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };

        let now = Instant::now();

        // 60 saniyeden eski kayıtları periyodik temizle
        if map.len() > 200 {
            map.retain(|_, time| now.duration_since(*time) < Duration::from_secs(60));
        }

        if let Some(prev_time) = map.get(fingerprint) {
            if now.duration_since(*prev_time) < self.ttl {
                // Çifte istek!
                return false;
            }
        }

        map.insert(fingerprint.to_string(), now);
        true
    }

    /// Fiş/Sipariş için benzersiz parmak izi üret
    pub fn generate_fingerprint(
        order_id: Option<&str>,
        table_name: Option<&str>,
        screen_id: Option<&str>,
        receipt_type: Option<&str>,
    ) -> String {
        format!(
            "ord:{}-tbl:{}-scr:{}-typ:{}",
            order_id.unwrap_or("none"),
            table_name.unwrap_or("none"),
            screen_id.unwrap_or("none"),
            receipt_type.unwrap_or("none")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dedup_suppresses_rapid_fire() {
        let dedup = DeduplicationRing::new(3);
        let fp = DeduplicationRing::generate_fingerprint(
            Some("ORD-101"),
            Some("Masa 4"),
            Some("kitchen"),
            Some("order"),
        );

        // İlk istek: Başarılı
        assert!(dedup.check_and_record(&fp));

        // 100ms sonra gelen aynı istek (ör. web veya mobilde çift tıklama): Engellenmeli!
        assert!(!dedup.check_and_record(&fp));
    }
}
