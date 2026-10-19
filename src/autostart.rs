use anyhow::Result;
use std::path::PathBuf;
use tracing::info;

pub struct AutostartManager;

impl AutostartManager {
    /// İşletim sistemine göre açılışta başlatma durumunu kontrol et
    pub fn is_autostart_enabled() -> bool {
        #[cfg(target_os = "macos")]
        {
            let plist_path = Self::get_macos_plist_path();
            plist_path.exists()
        }

        #[cfg(target_os = "windows")]
        {
            // Windows Registry Run key veya Startup shortcut kontrolü
            if let Some(startup_dir) = Self::get_windows_startup_path() {
                startup_dir.join("QORDY_Printer_Bridge.bat").exists()
            } else {
                false
            }
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            false
        }
    }

    /// Açılışta otomatik başlatmayı aktif veya pasif yap
    pub fn set_autostart(enable: bool) -> Result<()> {
        #[cfg(target_os = "macos")]
        {
            let plist_path = Self::get_macos_plist_path();
            if enable {
                if let Some(parent) = plist_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }

                let current_exe = std::env::current_exe()?;
                let exe_str = current_exe.to_string_lossy();

                let plist_content = format!(
                    r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.qordy.printerbridge</string>
    <key>ProgramArguments</key>
    <array>
        <string>{}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <false/>
    <key>StandardErrorPath</key>
    <string>/tmp/qordy_bridge_err.log</string>
    <key>StandardOutPath</key>
    <string>/tmp/qordy_bridge_out.log</string>
</dict>
</plist>"#,
                    exe_str
                );

                std::fs::write(&plist_path, plist_content)?;
                info!("macOS LaunchAgent kaydedildi: {:?}", plist_path);
            } else {
                if plist_path.exists() {
                    let _ = std::fs::remove_file(&plist_path);
                    info!("macOS LaunchAgent kaldırıldı: {:?}", plist_path);
                }
            }
        }

        #[cfg(target_os = "windows")]
        {
            if let Some(startup_dir) = Self::get_windows_startup_path() {
                let bat_path = startup_dir.join("QORDY_Printer_Bridge.bat");
                if enable {
                    let current_exe = std::env::current_exe()?;
                    let content = format!("@echo off\r\nstart \"\" \"{}\"\r\n", current_exe.to_string_lossy());
                    std::fs::write(&bat_path, content)?;
                    info!("Windows Başlangıç kısayolu kaydedildi: {:?}", bat_path);
                } else if bat_path.exists() {
                    let _ = std::fs::remove_file(&bat_path);
                    info!("Windows Başlangıç kısayolu silindi: {:?}", bat_path);
                }
            }
        }

        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn get_macos_plist_path() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        PathBuf::from(home)
            .join("Library")
            .join("LaunchAgents")
            .join("com.qordy.printerbridge.plist")
    }

    #[cfg(target_os = "windows")]
    fn get_windows_startup_path() -> Option<PathBuf> {
        std::env::var("APPDATA").ok().map(|appdata| {
            PathBuf::from(appdata)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs")
                .join("Startup")
        })
    }
}
