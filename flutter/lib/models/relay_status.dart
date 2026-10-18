import 'dart:convert';

/// Live tailcat relay probe from `tailcat-relay-status`.
class RelayStatus {
  final bool running;
  final String state;
  final String regionCode;
  final int? probeRttMs;
  final int? lastProbeMs;
  final String lastError;
  final String initStage;
  final String initError;
  final String rustInitError;

  const RelayStatus({
    required this.running,
    required this.state,
    this.regionCode = '',
    this.probeRttMs,
    this.lastProbeMs,
    this.lastError = '',
    this.initStage = '',
    this.initError = '',
    this.rustInitError = '',
  });

  bool get hasChip =>
      state == 'connecting' || state == 'reachable' || state == 'unreachable';

  /// Serve failure to show instead of "Generating pairing code...".
  /// Go's `init_stage` + `init_error` win; otherwise the Rust fallback.
  String? get initFailure {
    final go = initError.trim();
    if (go.isNotEmpty) {
      final stage = initStage.trim();
      return stage.isEmpty ? go : '$stage: $go';
    }
    final rust = rustInitError.trim();
    return rust.isEmpty ? null : rust;
  }

  static RelayStatus? fromJson(String raw) {
    final text = raw.trim();
    if (text.isEmpty) return null;
    final Object? decoded;
    try {
      decoded = jsonDecode(text);
    } catch (_) {
      return null;
    }
    if (decoded is! Map) return null;
    final map = decoded;
    String str(String key) {
      final value = map[key];
      return value is String ? value : '';
    }

    int? number(String key) {
      final value = map[key];
      if (value is int) return value;
      if (value is num) return value.round();
      return null;
    }

    final running = map['running'];
    return RelayStatus(
      running: running is bool ? running : false,
      state: str('state'),
      regionCode: str('region_code'),
      probeRttMs: number('probe_rtt_ms'),
      lastProbeMs: number('last_probe_ms'),
      lastError: str('last_error'),
      initStage: str('init_stage'),
      initError: str('init_error'),
      rustInitError: str('rust_init_error'),
    );
  }
}
