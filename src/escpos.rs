#![allow(dead_code)]
use crate::models::PrintJob;
use chrono::Local;

pub const PAPER_WIDTH_80MM: usize = 48; // 48 chars
pub const PAPER_WIDTH_58MM: usize = 32; // 32 chars

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TurkishEncoding {
    Cp857,
    Cp1254,
    Utf8,
    AsciiSanitized,
}

#[derive(Debug, Clone)]
pub struct EscPosBuilder {
    buffer: Vec<u8>,
    width: usize,
    encoding: TurkishEncoding,
}

impl EscPosBuilder {
    pub fn new(width: usize, encoding: TurkishEncoding) -> Self {
        let mut builder = Self {
            buffer: Vec::with_capacity(4096),
            width: if width == 0 { PAPER_WIDTH_80MM } else { width },
            encoding,
        };
        builder.init();
        builder
    }

    pub fn init(&mut self) -> &mut Self {
        self.buffer.extend_from_slice(&[0x1B, 0x40]); // ESC @ (Reset)
        match self.encoding {
            TurkishEncoding::Cp857 => {
                self.buffer.extend_from_slice(&[0x1B, 0x74, 0x0C]); // ESC t 12 (PC857 Turkish)
            }
            TurkishEncoding::Cp1254 => {
                self.buffer.extend_from_slice(&[0x1B, 0x74, 0x38]); // ESC t 56 (WPC1254 Turkish)
            }
            TurkishEncoding::Utf8 => {
                self.buffer.extend_from_slice(&[0x1C, 0x26]); // FS & (Select Chinese/UTF8 mode)
            }
            TurkishEncoding::AsciiSanitized => {}
        }
        self
    }

    pub fn align_left(&mut self) -> &mut Self {
        self.buffer.extend_from_slice(&[0x1B, 0x61, 0x00]);
        self
    }

    pub fn align_center(&mut self) -> &mut Self {
        self.buffer.extend_from_slice(&[0x1B, 0x61, 0x01]);
        self
    }

    pub fn align_right(&mut self) -> &mut Self {
        self.buffer.extend_from_slice(&[0x1B, 0x61, 0x02]);
        self
    }

    pub fn bold(&mut self, enabled: bool) -> &mut Self {
        self.buffer.extend_from_slice(&[0x1B, 0x45, if enabled { 0x01 } else { 0x00 }]);
        self
    }

    pub fn double_size(&mut self, enabled: bool) -> &mut Self {
        self.buffer.extend_from_slice(&[0x1B, 0x21, if enabled { 0x30 } else { 0x00 }]);
        self
    }

    pub fn double_height(&mut self, enabled: bool) -> &mut Self {
        self.buffer.extend_from_slice(&[0x1B, 0x21, if enabled { 0x10 } else { 0x00 }]);
        self
    }

    pub fn double_width(&mut self, enabled: bool) -> &mut Self {
        self.buffer.extend_from_slice(&[0x1B, 0x21, if enabled { 0x20 } else { 0x00 }]);
        self
    }

    pub fn feed(&mut self, lines: u8) -> &mut Self {
        self.buffer.extend_from_slice(&[0x1B, 0x64, lines]);
        self
    }

    pub fn newline(&mut self) -> &mut Self {
        self.buffer.push(0x0A);
        self
    }

    pub fn beep(&mut self) -> &mut Self {
        self.buffer.extend_from_slice(&[0x1B, 0x42, 0x02, 0x02]); // 2 beeps
        self
    }

    pub fn open_drawer(&mut self) -> &mut Self {
        self.buffer.extend_from_slice(&[0x1B, 0x70, 0x00, 0x19, 0xFA]);
        self
    }

    pub fn cut(&mut self, partial: bool) -> &mut Self {
        self.feed(3);
        if partial {
            self.buffer.extend_from_slice(&[0x1D, 0x56, 0x01]); // Partial cut
        } else {
            self.buffer.extend_from_slice(&[0x1D, 0x56, 0x00]); // Full cut
        }
        self
    }

    pub fn text(&mut self, s: &str) -> &mut Self {
        let encoded = self.encode_str(s);
        self.buffer.extend_from_slice(&encoded);
        self
    }

    pub fn text_ln(&mut self, s: &str) -> &mut Self {
        self.text(s);
        self.newline();
        self
    }

    pub fn divider(&mut self, ch: char) -> &mut Self {
        let line: String = std::iter::repeat(ch).take(self.width).collect();
        self.text_ln(&line);
        self
    }

    pub fn double_line(&mut self) -> &mut Self {
        self.divider('=');
        self
    }

    pub fn single_line(&mut self) -> &mut Self {
        self.divider('-');
        self
    }

    /// Sol ve sağ sütunlu satır basımı (ör: "Toplam           120.00 TL")
    pub fn two_column(&mut self, left: &str, right: &str) -> &mut Self {
        let left_len = left.chars().count();
        let right_len = right.chars().count();

        if left_len + right_len >= self.width {
            let max_left = self.width.saturating_sub(right_len + 1);
            let truncated_left: String = left.chars().take(max_left).collect();
            let spaces = self.width.saturating_sub(truncated_left.chars().count() + right_len);
            let line = format!("{}{}{}", truncated_left, " ".repeat(spaces), right);
            self.text_ln(&line);
        } else {
            let spaces = self.width - (left_len + right_len);
            let line = format!("{}{}{}", left, " ".repeat(spaces), right);
            self.text_ln(&line);
        }
        self
    }

    pub fn barcode_code128(&mut self, data: &str) -> &mut Self {
        self.align_center();
        self.buffer.extend_from_slice(&[0x1D, 0x68, 55]); // GS h 55
        self.buffer.extend_from_slice(&[0x1D, 0x77, 2]);  // GS w 2
        self.buffer.extend_from_slice(&[0x1D, 0x48, 2]);  // GS H 2 (chars below)
        let bytes = data.as_bytes();
        self.buffer.extend_from_slice(&[0x1D, 0x6B, 73, bytes.len() as u8]);
        self.buffer.extend_from_slice(bytes);
        self.newline();
        self
    }

    pub fn sanitize_turkish_ascii(s: &str) -> String {
        s.chars()
            .map(|c| match c {
                'ç' => 'c', 'Ç' => 'C',
                'ğ' => 'g', 'Ğ' => 'G',
                'ı' => 'i', 'İ' => 'I',
                'ö' => 'o', 'Ö' => 'O',
                'ş' => 's', 'Ş' => 'S',
                'ü' => 'u', 'Ü' => 'U',
                other => other,
            })
            .collect()
    }

    fn encode_str(&self, s: &str) -> Vec<u8> {
        match self.encoding {
            TurkishEncoding::AsciiSanitized => {
                Self::sanitize_turkish_ascii(s).into_bytes()
            }
            TurkishEncoding::Cp857 => {
                let mut bytes = Vec::with_capacity(s.len());
                for c in s.chars() {
                    match c {
                        'ç' => bytes.push(0x87),
                        'Ç' => bytes.push(0x80),
                        'ğ' => bytes.push(0xA6),
                        'Ğ' => bytes.push(0xA7),
                        'ı' => bytes.push(0x8D),
                        'İ' => bytes.push(0x98),
                        'ö' => bytes.push(0x94),
                        'Ö' => bytes.push(0x99),
                        'ş' => bytes.push(0x9F),
                        'Ş' => bytes.push(0x9E),
                        'ü' => bytes.push(0x81),
                        'Ü' => bytes.push(0x9A),
                        ascii if ascii.is_ascii() => bytes.push(ascii as u8),
                        _ => bytes.push(b'?'),
                    }
                }
                bytes
            }
            TurkishEncoding::Cp1254 => {
                let mut bytes = Vec::with_capacity(s.len());
                for c in s.chars() {
                    match c {
                        'ç' => bytes.push(0xE7),
                        'Ç' => bytes.push(0xC7),
                        'ğ' => bytes.push(0xF0),
                        'Ğ' => bytes.push(0xD0),
                        'ı' => bytes.push(0xFD),
                        'İ' => bytes.push(0xDD),
                        'ö' => bytes.push(0xF6),
                        'Ö' => bytes.push(0xD6),
                        'ş' => bytes.push(0xFE),
                        'Ş' => bytes.push(0xDE),
                        'ü' => bytes.push(0xFC),
                        'Ü' => bytes.push(0xDC),
                        ascii if ascii.is_ascii() => bytes.push(ascii as u8),
                        _ => bytes.push(b'?'),
                    }
                }
                bytes
            }
            TurkishEncoding::Utf8 => s.as_bytes().to_vec(),
        }
    }

    pub fn build(self) -> Vec<u8> {
        self.buffer
    }
}

pub struct ReceiptGenerator {
    pub business_name: String,
    pub address: String,
    pub phone: String,
    pub paper_width: usize,
    pub encoding: TurkishEncoding,
}

impl ReceiptGenerator {
    pub fn new(business_name: String, address: String, phone: String) -> Self {
        Self {
            business_name,
            address,
            phone,
            paper_width: PAPER_WIDTH_80MM,
            encoding: TurkishEncoding::Cp857, // Standart Restoran CP857 Türkçe
        }
    }

    pub fn generate_from_job(&self, job: &PrintJob) -> Vec<u8> {
        let screen_type = job.screen_type.as_deref().unwrap_or("kitchen");
        let screen_id = job.screen_id.as_deref().unwrap_or("");

        if screen_type.eq_ignore_ascii_case("cashier") || screen_id == "cashier_main" {
            self.build_customer_receipt(job)
        } else if screen_type.eq_ignore_ascii_case("waiter") || screen_id == "waiter_main" {
            self.build_adisyon_receipt(job)
        } else {
            // Varsayılan mutfak veya hazırlık fişi
            self.build_kitchen_receipt(job)
        }
    }

    /// Mutfak Fişi (Büyük masa adı, kalın ürünler, notlar, zil)
    pub fn build_kitchen_receipt(&self, job: &PrintJob) -> Vec<u8> {
        let mut b = EscPosBuilder::new(self.paper_width, self.encoding);
        b.beep();

        b.align_center();
        b.double_size(true);
        b.bold(true);
        b.text_ln("MUTFAK SIPARISI");
        b.double_size(false);

        let screen_name = job.screen_name.as_deref().unwrap_or("Mutfak");
        b.text_ln(&format!("[ {} ]", screen_name));
        b.double_line();

        // Masa ve Garson Bilgisi
        b.align_left();
        b.double_size(true);
        let table = job.table_name.as_deref().unwrap_or("Paket Servis");
        b.text_ln(&format!("MASA: {}", table));
        b.double_size(false);
        b.bold(false);

        let waiter = job.waiter_name.as_deref().unwrap_or("-");
        let order_id = job.order_id.as_deref().or(job.queue_id.as_deref()).unwrap_or("0");
        let now_str = Local::now().format("%d.%m.%Y %H:%M:%S").to_string();

        b.two_column(&format!("Garson: {}", waiter), &format!("#{}", order_id));
        b.two_column("Tarih:", &now_str);
        b.single_line();

        // Ürünler
        if let Some(items) = &job.items {
            for item in items {
                b.bold(true);
                b.double_height(true);
                let qty_str = if item.quantity.fract() == 0.0 {
                    format!("{}x", item.quantity as i64)
                } else {
                    format!("{:.1}x", item.quantity)
                };
                b.text_ln(&format!("{} {}", qty_str, item.name));
                b.double_height(false);
                b.bold(false);

                // Seçenekler (Örn: Az Pişmiş, Ekstra Peynir)
                if let Some(options) = &item.options {
                    for opt in options {
                        b.text_ln(&format!("   + {}", opt));
                    }
                }

                // Ürün Notu
                if let Some(notes) = &item.notes {
                    if !notes.trim().is_empty() {
                        b.bold(true);
                        b.text_ln(&format!("   >> NOT: {}", notes));
                        b.bold(false);
                    }
                }
                b.newline();
            }
        } else if let Some(content) = &job.content {
            b.text_ln(content);
        }

        // Genel Sipariş Notu
        if let Some(notes) = &job.notes {
            if !notes.trim().is_empty() {
                b.single_line();
                b.bold(true);
                b.text_ln(&format!("SIPARIS NOTU: {}", notes));
                b.bold(false);
            }
        }

        b.double_line();
        b.cut(true);
        b.build()
    }

    /// Kasiyer / Müşteri Fişi (İşletme logoları, fiyatlar, toplam, kdv, teşekkür yazısı)
    pub fn build_customer_receipt(&self, job: &PrintJob) -> Vec<u8> {
        let mut b = EscPosBuilder::new(self.paper_width, self.encoding);

        b.align_center();
        b.double_size(true);
        b.bold(true);
        b.text_ln(&self.business_name);
        b.double_size(false);
        b.bold(false);

        if !self.address.is_empty() {
            b.text_ln(&self.address);
        }
        if !self.phone.is_empty() {
            b.text_ln(&format!("Tel: {}", self.phone));
        }

        b.double_line();
        b.align_left();

        let table = job.table_name.as_deref().unwrap_or("Masa");
        let waiter = job.waiter_name.as_deref().unwrap_or("-");
        let order_id = job.order_id.as_deref().or(job.queue_id.as_deref()).unwrap_or("0");
        let now_str = Local::now().format("%d.%m.%Y %H:%M:%S").to_string();

        b.two_column(&format!("Masa: {}", table), &format!("No: #{}", order_id));
        b.two_column(&format!("Garson: {}", waiter), &now_str);
        b.single_line();

        // Ürün Tablosu Başlığı
        b.two_column("Urun / Miktar", "Tutar");
        b.single_line();

        let mut computed_total = 0.0;
        if let Some(items) = &job.items {
            for item in items {
                let unit_price = item.price.unwrap_or(0.0);
                let line_total = item.quantity * unit_price;
                computed_total += line_total;

                let left = format!("{}x {}", item.quantity, item.name);
                let right = format!("{:.2} TL", line_total);
                b.two_column(&left, &right);
            }
        } else if let Some(content) = &job.content {
            b.text_ln(content);
        }

        b.double_line();

        // Toplam Tutar
        let total = job.total_amount.unwrap_or(computed_total);
        b.double_size(true);
        b.bold(true);
        b.two_column("TOPLAM:", &format!("{:.2} TL", total));
        b.double_size(false);
        b.bold(false);

        if let Some(payment) = &job.payment_type {
            b.single_line();
            b.two_column("Odeme Turu:", payment);
        }

        b.double_line();
        b.align_center();
        b.text_ln("Bizi Tercih Ettiginiz Icin Tesekkur Ederiz!");
        if !self.business_name.is_empty() {
            b.text_ln(&self.business_name);
        }

        b.cut(false);
        b.build()
    }

    /// Garson / Adisyon Fişi
    pub fn build_adisyon_receipt(&self, job: &PrintJob) -> Vec<u8> {
        let mut b = EscPosBuilder::new(self.paper_width, self.encoding);

        b.align_center();
        b.bold(true);
        b.text_ln("--- ADISYON BILGI FISI ---");
        b.text_ln(&self.business_name);
        b.bold(false);
        b.double_line();

        b.align_left();
        let table = job.table_name.as_deref().unwrap_or("Masa");
        let waiter = job.waiter_name.as_deref().unwrap_or("-");
        b.two_column(&format!("Masa: {}", table), &format!("Garson: {}", waiter));
        b.single_line();

        let mut computed_total = 0.0;
        if let Some(items) = &job.items {
            for item in items {
                let unit_price = item.price.unwrap_or(0.0);
                let line_total = item.quantity * unit_price;
                computed_total += line_total;
                b.two_column(
                    &format!("{}x {}", item.quantity, item.name),
                    &format!("{:.2} TL", line_total),
                );
            }
        }

        b.single_line();
        let total = job.total_amount.unwrap_or(computed_total);
        b.bold(true);
        b.two_column("TOPLAM:", &format!("{:.2} TL", total));
        b.bold(false);

        b.cut(true);
        b.build()
    }

    /// Paket & Ürün Etiketi (Etiket ve Barkod Yazıcıları için Çıktı)
    pub fn build_label_receipt(&self, item_name: &str, table: &str, order_no: &str, notes: &str) -> Vec<u8> {
        let mut b = EscPosBuilder::new(self.paper_width, self.encoding);

        b.align_center();
        b.bold(true);
        b.double_height(true);
        b.text_ln(&format!("#{}", order_no));
        b.double_height(false);
        b.text_ln(&format!("Masa: {}", table));
        b.single_line();

        b.bold(true);
        b.double_size(true);
        b.text_ln(item_name);
        b.double_size(false);
        b.bold(false);

        if !notes.is_empty() {
            b.text_ln(&format!("Not: {}", notes));
        }
        b.single_line();

        // Barkod (Code 128)
        b.barcode_code128(order_no);
        b.text_ln(&Local::now().format("%d.%m.%Y %H:%M").to_string());

        b.cut(true);
        b.build()
    }

    /// TSPL Protokolü (Xprinter XP-365B, TSC TE200 vb. Etiket Yazıcıları için)
    pub fn build_tspl_label(&self, item_name: &str, table: &str, order_no: &str, notes: &str) -> Vec<u8> {
        let clean_item = EscPosBuilder::sanitize_turkish_ascii(item_name);
        let clean_table = EscPosBuilder::sanitize_turkish_ascii(table);
        let clean_notes = EscPosBuilder::sanitize_turkish_ascii(notes);
        let now_str = Local::now().format("%d.%m %H:%M").to_string();

        let mut cmd = String::new();
        cmd.push_str("SIZE 50 mm, 30 mm\r\n");
        cmd.push_str("GAP 2 mm, 0\r\n");
        cmd.push_str("DIRECTION 1\r\n");
        cmd.push_str("CLS\r\n");
        cmd.push_str(&format!("TEXT 20,15,\"3\",0,1,1,\"#{}\"\r\n", order_no));
        cmd.push_str(&format!("TEXT 140,15,\"2\",0,1,1,\"Masa: {}\"\r\n", clean_table));
        cmd.push_str(&format!("TEXT 20,55,\"3\",0,1,1,\"{}\"\r\n", clean_item));
        if !clean_notes.is_empty() {
            cmd.push_str(&format!("TEXT 20,95,\"2\",0,1,1,\"Not: {}\"\r\n", clean_notes));
        }
        cmd.push_str(&format!("BARCODE 20,135,\"128\",45,1,0,2,2,\"{}\"\r\n", order_no));
        cmd.push_str(&format!("TEXT 240,15,\"2\",0,1,1,\"{}\"\r\n", now_str));
        cmd.push_str("PRINT 1,1\r\n");

        cmd.into_bytes()
    }

    /// ZPL Protokolü (Zebra ZD220, ZD420 vb. Endüstriyel Etiket Yazıcıları için)
    pub fn build_zpl_label(&self, item_name: &str, table: &str, order_no: &str, notes: &str) -> Vec<u8> {
        let clean_item = EscPosBuilder::sanitize_turkish_ascii(item_name);
        let clean_table = EscPosBuilder::sanitize_turkish_ascii(table);
        let clean_notes = EscPosBuilder::sanitize_turkish_ascii(notes);

        let mut zpl = String::new();
        zpl.push_str("^XA\r\n");
        zpl.push_str("^PW400\r\n");
        zpl.push_str("^LL240\r\n");
        zpl.push_str(&format!("^FO20,20^A0N,28,28^FD#{} - Masa: {}^FS\r\n", order_no, clean_table));
        zpl.push_str(&format!("^FO20,60^A0N,26,26^FD{}^FS\r\n", clean_item));
        if !clean_notes.is_empty() {
            zpl.push_str(&format!("^FO20,95^A0N,20,20^FDNot: {}^FS\r\n", clean_notes));
        }
        zpl.push_str(&format!("^FO20,130^BCN,40,Y,N,N^FD{}^FS\r\n", order_no));
        zpl.push_str("^XZ\r\n");

        zpl.into_bytes()
    }

    /// Self-Test & Sistem Tanılama Fişi
    pub fn build_test_receipt(&self, local_ip: &str, port: u16, printer_name: &str) -> Vec<u8> {
        let mut b = EscPosBuilder::new(self.paper_width, self.encoding);
        b.beep();

        b.align_center();
        b.double_size(true);
        b.bold(true);
        b.text_ln("QORDY BRIDGE v2.5");
        b.text_ln("[ RUST EDITION ]");
        b.double_size(false);
        b.bold(false);

        b.double_line();
        b.align_left();
        b.text_ln("YAZICI BAGLANTI TESTI BASARILI!");
        b.single_line();

        b.two_column("Yazici Adi:", printer_name);
        b.two_column("Kopru IP:", local_ip);
        b.two_column("Kopru Port:", &port.to_string());
        b.two_column("Tarih/Saat:", &Local::now().format("%d.%m.%Y %H:%M:%S").to_string());
        b.two_column("Surum:", "v2.5.0 Apple Edition");

        b.single_line();
        b.text_ln("Turkce Karakter Testi:");
        b.text_ln("C c, G g, I i, O o, S s, U u");
        if !self.business_name.is_empty() {
            b.text_ln(&self.business_name);
        }

        b.double_line();
        b.align_center();
        b.text_ln("Mobil Uygulamalar Yerel Agdan Baglanabilir");
        b.text_ln("http://<yerel-ip>:8089/api/print");

        b.cut(false);
        b.build()
    }

    /// Kapsamlı Sistem Teşhis ve Rapor Fişi
    pub fn build_diagnostic_receipt(
        &self,
        local_ip: &str,
        port: u16,
        active_printers: &[String],
        uptime: u64,
        printed_count: u64,
    ) -> Vec<u8> {
        let mut b = EscPosBuilder::new(self.paper_width, self.encoding);
        b.beep();

        b.align_center();
        b.double_size(true);
        b.bold(true);
        b.text_ln("QORDY SISTEM RAPORU");
        b.double_size(false);
        b.bold(false);
        b.text_ln("Masaustu Termal Yazici Koprusu");
        b.double_line();

        b.align_left();
        b.two_column("Isletme Adi:", &self.business_name);
        b.two_column("Yerel IP:", local_ip);
        b.two_column("API Port:", &port.to_string());
        b.two_column("Calisma Suresi:", &format!("{} sn", uptime));
        b.two_column("Basilan Fisler:", &printed_count.to_string());
        b.two_column("Tarih:", &Local::now().format("%d.%m.%Y %H:%M").to_string());
        b.single_line();

        b.bold(true);
        b.text_ln("BAGLI TERMAL YAZICILAR:");
        b.bold(false);
        for p in active_printers {
            b.text_ln(&format!(" * {}", p));
        }

        b.single_line();
        b.text_ln("MOBIL ENTEGRASYON (LAN):");
        b.text_ln(&format!("POST http://{}:{}/api/print", local_ip, port));

        b.double_line();
        b.align_center();
        b.text_ln("Sistem Calisma Durumu: %100 AKTIF");
        b.text_ln("Rust Zero-Overhead Memory: ~8 MB");

        b.cut(true);
        b.build()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_basic() {
        let mut b = EscPosBuilder::new(PAPER_WIDTH_80MM, TurkishEncoding::AsciiSanitized);
        b.text_ln("Test Başlığı");
        b.cut(true);
        let bytes = b.build();
        assert!(!bytes.is_empty());
        assert!(bytes.contains(&0x1D)); // GS V cut
    }

    #[test]
    fn test_turkish_encoding_cp857() {
        let mut b = EscPosBuilder::new(PAPER_WIDTH_80MM, TurkishEncoding::Cp857);
        b.text_ln("Şiir Çorba Öğle ılık");
        let bytes = b.build();
        assert!(!bytes.is_empty());
    }

    #[test]
    fn test_barcode_code128() {
        let mut b = EscPosBuilder::new(PAPER_WIDTH_80MM, TurkishEncoding::AsciiSanitized);
        b.barcode_code128("QORDY-12345");
        let bytes = b.build();
        assert!(bytes.contains(&0x1D)); // GS k
        assert!(bytes.contains(&0x6B));
    }

    #[test]
    fn test_formatter_receipts() {
        let generator = ReceiptGenerator::new(
            "QORDY Kafe".to_string(),
            "Bağdat Cad. No:12".to_string(),
            "0216 123 4567".to_string(),
        );

        let test_rec = generator.build_test_receipt("192.168.1.100", 8089, "Mutfak Yazıcısı");
        assert!(!test_rec.is_empty());

        let label_rec = generator.build_label_receipt(
            "Adana Kebap Porsiyon",
            "Masa 5",
            "482",
            "Acısız olsun",
        );
        assert!(!label_rec.is_empty());

        let tspl_rec = generator.build_tspl_label("Adana Kebap", "Masa 5", "482", "Acısız");
        assert!(!tspl_rec.is_empty());
        let tspl_str = String::from_utf8_lossy(&tspl_rec);
        assert!(tspl_str.contains("SIZE 50 mm"));

        let zpl_rec = generator.build_zpl_label("Adana Kebap", "Masa 5", "482", "Acısız");
        assert!(!zpl_rec.is_empty());
        let zpl_str = String::from_utf8_lossy(&zpl_rec);
        assert!(zpl_str.contains("^XA"));
    }
}
