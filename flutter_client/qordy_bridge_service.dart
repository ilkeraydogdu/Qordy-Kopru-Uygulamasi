import 'dart:async';
import 'dart:convert';
import 'package:http/http.dart' as http;

/// QORDY Akıllı Yazıcı Köprüsü - Mobil İstemci Servisi (Flutter / iOS / Android)
///
/// Bu servis, garson veya kasiyer telefonunun yerel Wi-Fi ağı üzerinden
/// Rust ile yazılmış köprüye doğrudan (50ms altında) fiş göndermesini sağlar.
/// İnternet bağlantısı kesilse bile yerel ağda yazdırmaya devam eder!
class QordyBridgeClient {
  final String bridgeHost; // Örn: "192.168.1.50" veya "localhost"
  final int bridgePort; // Varsayılan: 8089
  final Duration timeout;

  QordyBridgeClient({
    required this.bridgeHost,
    this.bridgePort = 8089,
    this.timeout = const Duration(seconds: 3),
  });

  String get baseUrl => 'http://$bridgeHost:$bridgePort';

  /// Köprünün sağlık durumunu ve aktif yazıcılarını kontrol et
  Future<bool> checkHealth() async {
    try {
      final res = await http.get(Uri.parse('$baseUrl/api/status')).timeout(timeout);
      if (res.statusCode == 200) {
        final data = jsonDecode(res.body);
        return data['version'] != null;
      }
    } catch (_) {}
    return false;
  }

  /// Aktif yazıcıların listesini getir
  Future<List<Map<String, dynamic>>> getPrinters() async {
    try {
      final res = await http.get(Uri.parse('$baseUrl/api/printers')).timeout(timeout);
      if (res.statusCode == 200) {
        final data = jsonDecode(res.body);
        if (data['success'] == true && data['printers'] is List) {
          return List<Map<String, dynamic>>.from(data['printers']);
        }
      }
    } catch (e) {
      print('[QordyBridge] Yazıcı listesi alınamadı: $e');
    }
    return [];
  }

  /// Doğrudan Yerel Fiş Basma (LAN Direct Print)
  ///
  /// Sipariş onaylandığı anda mutfak veya kasadaki yazıcıya milisaniyeler içinde basar.
  Future<QordyPrintResult> printOrder({
    required String screenId, // 'kitchen_main', 'cashier_main', 'waiter_main'
    required String orderId,
    required String tableName,
    required String waiterName,
    required List<QordyOrderItem> items,
    double? totalAmount,
    String? paymentType,
    String? notes,
  }) async {
    final payload = {
      'screen_id': screenId,
      'order_id': orderId,
      'table_name': tableName,
      'waiter_name': waiterName,
      'items': items.map((i) => i.toJson()).toList(),
      'total_amount': totalAmount,
      'payment_type': paymentType,
      'notes': notes,
    };

    try {
      final res = await http
          .post(
            Uri.parse('$baseUrl/api/print'),
            headers: {'Content-Type': 'application/json'},
            body: jsonEncode(payload),
          )
          .timeout(timeout);

      final data = jsonDecode(res.body);
      if (res.statusCode == 200 && data['success'] == true) {
        return QordyPrintResult(
          success: true,
          jobId: data['job_id'] ?? '',
          targetPrinter: data['target_printer'] ?? '',
          message: data['message'] ?? 'Yazdırıldı',
        );
      } else {
        return QordyPrintResult(
          success: false,
          error: data['error'] ?? 'Yazdırma hatası: ${res.statusCode}',
        );
      }
    } catch (e) {
      return QordyPrintResult(
        success: false,
        error: 'Köprüye ulaşılamadı (Yerel Ağ Hatası): $e',
      );
    }
  }

  /// Test Fişi Bas
  Future<bool> printTestReceipt([String printerId = 'printer-kitchen']) async {
    try {
      final res = await http.post(
        Uri.parse('$baseUrl/api/test-print'),
        headers: {'Content-Type': 'application/json'},
        body: jsonEncode({'printer_id': printerId}),
      ).timeout(timeout);
      return res.statusCode == 200;
    } catch (_) {
      return false;
    }
  }
}

class QordyOrderItem {
  final String name;
  final double quantity;
  final double? price;
  final String? notes;
  final List<String>? options;

  QordyOrderItem({
    required this.name,
    required this.quantity,
    this.price,
    this.notes,
    this.options,
  });

  Map<String, dynamic> toJson() => {
        'name': name,
        'quantity': quantity,
        if (price != null) 'price': price,
        if (notes != null) 'notes': notes,
        if (options != null) 'options': options,
      };
}

class QordyPrintResult {
  final bool success;
  final String? jobId;
  final String? targetPrinter;
  final String? message;
  final String? error;

  QordyPrintResult({
    required this.success,
    this.jobId,
    this.targetPrinter,
    this.message,
    this.error,
  });
}
