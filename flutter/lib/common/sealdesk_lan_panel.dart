import 'dart:async';
import 'dart:convert';
import 'package:flutter/material.dart';
import '../common.dart';
import '../models/platform_model.dart';

class SealDeskLanPanel extends StatefulWidget {
  final bool host;
  final Future<void> Function(String address) connectPeer;
  const SealDeskLanPanel(
      {super.key, required this.host, required this.connectPeer});
  @override
  State<SealDeskLanPanel> createState() => _SealDeskLanPanelState();
}

class _SealDeskLanPanelState extends State<SealDeskLanPanel> {
  Timer? _timer;
  bool _loading = false;
  bool _ready = false;
  String? _error;
  List<dynamic> _peers = [];
  @override
  void initState() {
    super.initState();
    _poll();
    _timer = Timer.periodic(const Duration(seconds: 3), (_) => _poll());
  }

  @override
  void didUpdateWidget(covariant SealDeskLanPanel oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.host != widget.host) {
      _peers = [];
      _poll();
    }
  }

  @override
  void dispose() {
    _timer?.cancel();
    super.dispose();
  }

  Future<void> _poll() async {
    if (_loading) return;
    _loading = true;
    final host = widget.host;
    try {
      final json = await bind.mainGetOption(
          key: host ? 'sealdesk-lan-trusted' : 'sealdesk-lan-peers');
      final peers =
          json.isEmpty ? <dynamic>[] : jsonDecode(json) as List<dynamic>;
      final ready =
          host && await bind.mainGetOption(key: 'sealdesk-lan-ready') == 'Y';
      if (mounted && host == widget.host) {
        setState(() {
          _peers = peers;
          _ready = ready;
          _error = null;
        });
      }
    } catch (_) {
      if (mounted) {
        setState(() =>
            _error = translate("Unable to read LAN status. Please try again"));
      }
    } finally {
      _loading = false;
    }
  }

  Future<void> _forget(Map<String, dynamic> peer) async {
    await bind.mainSetOption(
        key: 'sealdesk-lan-forget', value: peer['key'] as String);
    final json = await bind.mainGetOption(key: 'sealdesk-lan-trusted');
    final peers =
        json.isEmpty ? <dynamic>[] : jsonDecode(json) as List<dynamic>;
    if (mounted) {
      setState(() {
        _peers = peers;
        _error = null;
      });
    }
    if (mounted && peers.any((p) => p['key'] == peer['key'])) {
      setState(() =>
          _error = translate("Unable to revoke access. Please try again"));
    }
  }

  @override
  Widget build(BuildContext context) =>
      Column(mainAxisSize: MainAxisSize.min, children: [
        const Divider(),
        Row(mainAxisAlignment: MainAxisAlignment.center, children: [
          Text(widget.host
              ? (_ready
                  ? translate("LAN sharing is ready")
                  : translate("Starting LAN sharing..."))
              : translate("LAN devices")),
          IconButton(
              tooltip: translate('Refresh'),
              onPressed: _poll,
              icon: const Icon(Icons.refresh)),
        ]),
        if (widget.host)
          Text(
              translate(
                  "Remembered controllers connect automatically. Revoke access to require approval again."),
              style: TextStyle(fontSize: 12)),
        if (!widget.host && _peers.isEmpty)
          Text(
              translate("Looking for devices sharing on your local network..."),
              style: TextStyle(fontSize: 12)),
        for (final item in _peers)
          ListTile(
              dense: true,
              title: Text(item['name'] as String? ?? ''),
              subtitle: Text(widget.host
                  ? (item['key'] as String).substring(0, 12)
                  : item['ip'] as String),
              trailing: TextButton(
                  onPressed: () async {
                    if (widget.host) {
                      await _forget(Map<String, dynamic>.from(item));
                    } else {
                      await widget.connectPeer(item['address'] as String);
                    }
                  },
                  child: Text(widget.host
                      ? translate("Revoke access")
                      : translate('Connect')))),
        if (_error != null)
          Text(_error!,
              style: TextStyle(color: Theme.of(context).colorScheme.error)),
      ]);
}
