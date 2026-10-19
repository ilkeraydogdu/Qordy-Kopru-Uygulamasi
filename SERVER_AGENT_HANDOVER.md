# 🚀 QORDY Smart Print Bridge (Rust Native Edition v2.5.0)
## Sunucu & Antigravity Ajanı Entegrasyon ve Kontrol Raporu (Handover Document)

Bu doküman, sunucuda (`194.110.169.165` - `qordy.com`) çalışan Antigravity ve Claude yapay zeka ajanları ile yerel masaüstü istemcisi arasındaki tam uyumluluğu, API sözleşmelerini ve kurumsal dağıtım yapısını açıklar.

---

### 1. Sistem Mimarisi & Başarılanlar
Eski Python tabanlı hantal köprü (~150MB RAM, 40MB boyut, gecikmeli baskı) tamamen kaldırılmış; **Rust, Tokio Asenkron Motoru, Axum Web/WebSocket Sunucusu, Wry WebKit Motoru ve Tao Pencere Yöneticisi** ile sıfırdan kurumsal düzeyde yazılmıştır.

- **İkili (Binary) Boyutu:** 
  - macOS: `4.1 MB` (`qordy_bridge_mac`)
  - Windows: `4.8 MB` (`qordy_bridge_rs.exe`)
- **Bellek (RAM) Kullanımı:** Yalnızca `~8 MB` (Eski sürüme göre %95 tasarruf)
- **Tasarım:** Apple HIG (Human Interface Guidelines) karanlık mod, modern cam morfizm (glassmorphism), özel SVG donanım ikonları ve animasyonlar.
- **Pencere Kapatma Koruması:** Kullanıcı `X` (Kapat) butonuna bastığında uygulama kapanmaz; Tao `WindowEvent::CloseRequested` olayı yakalanarak pencere sistem tepsisine (`TrayIcon`) gizlenir ve arka planda yazdırma servisi 7/24 kesintisiz çalışmaya devam eder. Menüden veya simgeye tıklanarak pencere yeniden öne getirilebilir.

---

### 2. Doğrulanan ve Tam Uyumlu Sunucu Uç Noktaları (`https://qordy.com/pb/`)

Sunucudaki `app/controllers/PrinterBridgeController.php` ve `app/config/routes.php` ile test edilip %100 canlı çalışan uç noktalar:

| Uç Nokta | HTTP Metodu | İstek Gövdesi / Parametreler | Yanıt / Durum |
|---|---|---|---|
| `/pb/register` | `POST` | `{"config_code": "...", "bridge_id": "...", ...}` | İşletme bilgileri, logo URL (`caddecafe_logo.webp`), şube kodu (#296466) ve API anahtarını döner. |
| `/pb/heartbeat` | `POST` | `{"bridge_id": "...", "api_key": "..."}` | Canlılık sinyali gönderir, köprüyü veritabanında `ONLINE` yapar. (Doğrulandı: `{"success":true}`) |
| `/pb/queue` | `POST` | `{"bridge_id": "...", "api_key": "...", "limit": 20}` | Şubeye ait basılmayı bekleyen fiş kuyruğunu döner. (Doğrulandı: `{"success":true,"jobs":[],"count":0}`) |
| `/pb/screens` | `GET` | `?bridge_id=...&api_key=...` | Şubenin mutfak/hazırlık ekranlarını döner (Nargile, Mutfak, Garson, Kasiyer). |
| `/pb/update-status` | `POST` | `{"bridge_id": "...", "api_key": "...", "queue_id": "...", "status": "PRINTED"}` | Yazdırılan veya hata alan işi günceller. |

---

### 3. Canlı İşletme Entegrasyonu: Cadde Cafe
Mock (sahte) veriler tamamen kaldırılmıştır. Canlı işletme konfigürasyonu:
- **İşletme:** Cadde Cafe
- **İşletme / Şube Kodu:** `#296466`
- **Şube:** Ana Şube
- **Adres:** Caferağa Mah. Moda Cad. No:42 Kadıköy / İstanbul
- **Logo:** `https://qordy.com/assets/images/business/product/caddecafe_logo.webp`
- **Config Key:** `dd3078b238e59bf2869994f918bb4a3c903d4c10ba5ebdcd078812bcb89bd5f5`

---

### 4. Tek Tıkla Kurumsal Dağıtım & İndirme Linkleri

Derlenen standalone ikili dosyalar doğrudan sunucunun public dağıtım dizinine yüklenmiştir:
- **Windows İndirme:** `https://qordy.com/download/qordy_bridge_rs.exe` (Ayrıca `QORDY_Printer_Bridge_Setup.exe` güncellenmiştir)
- **macOS İndirme:** `https://qordy.com/download/qordy_bridge_mac`

**Terminalden Tek Satırda Kurulum:**
- macOS / Linux:
  ```bash
  curl -sSL https://raw.githubusercontent.com/ilkeraydogdu/Qordy-Kopru-Uygulamasi/main/scripts/install.sh | bash
  ```
- Windows PowerShell:
  ```powershell
  irm https://raw.githubusercontent.com/ilkeraydogdu/Qordy-Kopru-Uygulamasi/main/scripts/install.ps1 | iex
  ```

---

### 5. Git Deposu & CI/CD
- **GitHub Deposu:** `git@github.com:ilkeraydogdu/Qordy-Kopru-Uygulamasi.git`
- **Ana Dal (Branch):** `main`
- **Sunucu Klon Yolu:** `/var/www/vhosts/qordy.com/httpdocs/MasaustuApp_v3_rust_bridge`
- **Otomasyon:** `.github/workflows/release.yml` ile her push ve etiketlemede Windows ve macOS ikilileri GitHub Actions üzerinden otonom olarak derlenip paketlenir.
