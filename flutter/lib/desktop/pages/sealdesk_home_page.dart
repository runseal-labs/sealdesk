import 'dart:async';

import 'package:desktop_drop/desktop_drop.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:qr_flutter/qr_flutter.dart';

import '../../common.dart';
import '../../common/sealdesk_pairing.dart';
import '../../models/platform_model.dart';
import '../../models/relay_status.dart';

const _defaultDerpMapUrl = 'https://tailcat.dev/derpmap.json';

String sealDeskInitialPage(String saved) =>
    saved == 'control' ? 'control' : 'host';

class SealDeskHostState {
  final String address;
  final String? password;
  final String name;
  final List<String> connectedNames;
  final int connectedCount;
  final bool stopped;
  final bool permissionMissing;
  final String startError;
  final RelayStatus? relayStatus;

  const SealDeskHostState({
    required this.address,
    required this.password,
    required this.name,
    this.connectedNames = const [],
    this.connectedCount = 0,
    this.stopped = false,
    this.permissionMissing = false,
    this.startError = '',
    this.relayStatus,
  });
}

enum SealDeskSharePhase { stopped, permissionMissing, starting, running, error }

class SealDeskHomePage extends StatefulWidget {
  final Widget Function(bool host)? lanPanelBuilder;
  final String initialMode;
  final String? initialInput;
  final Future<SealDeskHostState> Function() loadHost;
  final Future<void> Function() refreshPassword;
  final Future<void> Function(SealDeskPairing) connect;
  final Future<void> Function(String) saveMode;
  final int Function()? now;
  final Future<SealDeskPairing?> Function()? pickImage;
  final Future<SealDeskPairing> Function(String)? loadImageFile;
  final Future<void> Function()? resetAddress;
  final Future<SealDeskPairing> Function()? pasteImage;
  final Future<void> Function()? onStart;
  final Future<void> Function()? onStop;
  final Widget? permissionBanner;
  final Widget? shareSettings;
  final Widget? footer;
  final bool hostOnly;

  const SealDeskHomePage({
    super.key,
    required this.initialMode,
    this.lanPanelBuilder,
    this.initialInput,
    required this.loadHost,
    required this.refreshPassword,
    required this.connect,
    required this.saveMode,
    this.now,
    this.pickImage,
    this.loadImageFile,
    this.resetAddress,
    this.pasteImage,
    this.onStart,
    this.onStop,
    this.permissionBanner,
    this.shareSettings,
    this.footer,
    this.hostOnly = false,
  });

  @override
  State<SealDeskHomePage> createState() => _SealDeskHomePageState();
}

class _SealDeskHomePageState extends State<SealDeskHomePage> {
  final _input = TextEditingController();
  Timer? _timer;
  late String _mode;
  SealDeskPairing? _pairing;
  String? _code;
  String? _revokedAddress;
  String _status = 'Generating pairing code...';
  RelayStatus? _relay;

  String? _error;
  String? _hostError;
  SealDeskSharePhase _phase = SealDeskSharePhase.starting;
  bool _hidden = false;
  bool _loading = false;
  bool _busy = false;

  int _now() =>
      widget.now?.call() ?? DateTime.now().millisecondsSinceEpoch ~/ 1000;

  @override
  void initState() {
    super.initState();
    _mode = widget.hostOnly ? 'host' : sealDeskInitialPage(widget.initialMode);
    _poll();
    _timer = Timer.periodic(const Duration(seconds: 1), (_) => _poll());
    if (_mode == 'control' && widget.initialInput != null) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted) _importStartup(widget.initialInput!);
      });
    }
  }

  Future<void> _importStartup(String input) => _act(() async {
        final SealDeskPairing pairing;
        if (SealDeskPairing.isPairingCode(input)) {
          pairing = SealDeskPairing.parse(input, now: _now());
        } else {
          final loader = widget.loadImageFile;
          if (loader == null) {
            throw FormatException(
                translate('This version cannot import QR images'));
          }
          pairing = await loader(input);
        }
        if (mounted) await _joinPairing(pairing);
      });

  @override
  void dispose() {
    _timer?.cancel();
    _input.dispose();
    super.dispose();
  }

  Future<void> _poll({bool force = false}) async {
    if (_loading || _mode != 'host' || (_busy && !force)) return;
    _loading = true;
    try {
      final host = await widget.loadHost();
      if (!mounted || _mode != 'host' || (_busy && !force)) return;
      if (host.startError.isNotEmpty) {
        setState(() {
          _phase = SealDeskSharePhase.error;
          _pairing = null;
          _code = null;
          _relay = null;
          _hostError = host.startError;
        });
        return;
      }
      _hostError = null;
      if (host.address == _revokedAddress) {
        setState(() {
          _phase = SealDeskSharePhase.starting;
          _pairing = null;
          _code = null;
          _relay = null;
          _status = 'Updating QR code...';
        });
        return;
      }
      if (host.permissionMissing) {
        setState(() {
          _phase = SealDeskSharePhase.permissionMissing;
          _pairing = null;
          _code = null;
          _relay = null;
        });
        return;
      }
      if (host.stopped || !SealDeskPairing.isAddress(host.address)) {
        setState(() {
          _pairing = null;
          _code = null;
          _relay = null;
          if (host.stopped) {
            _phase = SealDeskSharePhase.stopped;
            _status = 'Sharing stopped';
          } else {
            _phase = SealDeskSharePhase.starting;
            _status = 'Generating pairing code...';
          }
        });
        return;
      }
      final now = _now();
      _revokedAddress = null;
      if (_pairing == null ||
          _pairing!.address != host.address ||
          _pairing!.password != host.password ||
          _pairing!.name != host.name ||
          _pairing!.expiresAt! <= now) {
        _pairing = SealDeskPairing(
          address: host.address,
          password: host.password,
          name: host.name,
          expiresAt: now + 600,
        );
        _code = _pairing!.encode(now: now);
      }
      setState(() {
        _phase = SealDeskSharePhase.running;
        _relay = host.relayStatus;
        _status = 'Sharing';
      });
    } catch (_) {
      if (mounted && _mode == 'host') {
        setState(() {
          _phase = SealDeskSharePhase.error;
          _pairing = null;
          _code = null;
          _relay = null;
          _status = 'Unable to get pairing code, please try again';
          _hostError = translate(_status);
        });
      }
    } finally {
      _loading = false;
    }
  }

  Future<void> _selectMode(String mode) async {
    await _act(() async {
      await widget.saveMode(mode);
      if (!mounted) return;
      setState(() {
        _mode = mode;
        _phase = SealDeskSharePhase.starting;
        _pairing = null;
        _code = null;
        _hostError = null;
        _relay = null;
        _status = 'Generating pairing code...';
      });
      await _poll(force: true);
    });
  }

  Future<void> _retryHost() => _act(() async {
        await widget.saveMode('control');
        await widget.saveMode('host');
        if (mounted) await _poll(force: true);
      });

  Future<void> _resetAddress() => _act(() async {
        final confirmed = await showDialog<bool>(
          context: context,
          builder: (context) => AlertDialog(
            title: Text(translate('Reset device address?')),
            content: Text(translate(
                'This disconnects the current session and invalidates all old QR codes. Share the new code with the other person.')),
            actions: [
              TextButton(
                onPressed: () => Navigator.pop(context, false),
                child: Text(translate('Cancel')),
              ),
              FilledButton(
                onPressed: () => Navigator.pop(context, true),
                child: Text(translate('Confirm reset')),
              ),
            ],
          ),
        );
        if (confirmed != true || !mounted) return;
        setState(() {
          _phase = SealDeskSharePhase.starting;
          _revokedAddress = _pairing?.address ?? _revokedAddress;
          _pairing = null;
          _code = null;
          _relay = null;
          _status = 'Resetting address...';
        });
        await widget.resetAddress!();
        if (mounted) await _poll(force: true);
      });

  Future<void> _act(Future<void> Function() operation) async {
    if (_busy) return;
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      await operation();
    } on FormatException catch (error) {
      if (mounted) setState(() => _error = translate(error.message));
    } catch (_) {
      if (mounted) {
        setState(() => _error = translate('Operation failed, please try again'));
      }
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _connect() => _act(() async {
        final pairing = SealDeskPairing.parse(_input.text, now: _now());
        await _joinPairing(pairing);
      });

  Future<void> _joinPairing(SealDeskPairing pairing) async {
    pairing.encode(now: _now());
    await widget.connect(pairing);
    if (mounted) _input.clear();
  }

  Future<void> _pickImage() => _act(() async {
        final pairing = await widget.pickImage!();
        if (mounted && pairing != null) await _joinPairing(pairing);
      });

  Widget _buildQr() {
    final validation = QrValidator.validate(
      data: _code!,
      version: QrVersions.auto,
      errorCorrectionLevel: QrErrorCorrectLevel.M,
    );
    final qr = validation.qrCode;
    if (qr == null) {
      return Text(translate(
          'Pairing code is too long for a QR image. Copy the text instead.'));
    }
    final quietZone = (280 * 4 / (qr.moduleCount + 8)).ceilToDouble();
    return QrImageView.withQr(
      key: const ValueKey('sealdesk-qr'),
      qr: qr,
      errorCorrectionLevel: QrErrorCorrectLevel.M,
      size: 280,
      padding: EdgeInsets.all(quietZone),
      backgroundColor: Colors.white,
      eyeStyle: const QrEyeStyle(color: Colors.black),
      dataModuleStyle: const QrDataModuleStyle(color: Colors.black),
    );
  }

  @override
  Widget build(BuildContext context) {
    final page = Center(
        child: SingleChildScrollView(
          padding: const EdgeInsets.fromLTRB(16, 8, 16, 24),
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 520),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                if (!widget.hostOnly) ...[
                  Wrap(
                    spacing: 12,
                    runSpacing: 12,
                    alignment: WrapAlignment.center,
                    children: [
                      ChoiceChip(
                        label: Text(translate('Share this device')),
                        selected: _mode == 'host',
                        onSelected: _busy ? null : (_) => _selectMode('host'),
                      ),
                      ChoiceChip(
                        label: Text(translate('Connect to a device')),
                        selected: _mode == 'control',
                        onSelected:
                            _busy ? null : (_) => _selectMode('control'),
                      ),
                    ],
                  ),
                  const SizedBox(height: 24),
                ],
                if (_mode.isEmpty)
                  Text(translate(
                      'Choose how to use SealDesk. Your choice will be remembered.')),
                if (_mode == 'host') ...[
                  ..._buildHost(),
                  if (_phase == SealDeskSharePhase.running &&
                      widget.lanPanelBuilder != null) ...[
                    const SizedBox(height: 12),
                    widget.lanPanelBuilder!(true),
                  ],
                  if (!widget.hostOnly) ...[
                    const SizedBox(height: 12),
                    Text(translate(
                        'Switching will disconnect the current session')),
                  ],
                ],
                if (_mode == 'control') ...[
                  ..._buildControl(),
                  if (widget.lanPanelBuilder != null) ...[
                    const SizedBox(height: 20),
                    widget.lanPanelBuilder!(false),
                  ],
                ],
                if (_error != null)
                  Padding(
                    padding: const EdgeInsets.only(top: 16),
                    child: Text(_error!,
                        key: const ValueKey('sealdesk-error'),
                        style: TextStyle(
                            color: Theme.of(context).colorScheme.error)),
                  ),
                if (widget.footer != null) widget.footer!,
                if (_mode == 'host' &&
                    _phase == SealDeskSharePhase.running &&
                    widget.onStop != null)
                  Padding(
                    padding: const EdgeInsets.only(top: 16),
                    child: SizedBox(
                      width: double.infinity,
                      child: OutlinedButton.icon(
                        key: const ValueKey('sealdesk-stop-service'),
                        style: OutlinedButton.styleFrom(
                          foregroundColor:
                              Theme.of(context).colorScheme.error,
                          side: BorderSide(
                              color: Theme.of(context).colorScheme.error),
                        ),
                        onPressed:
                            _busy ? null : () => _act(widget.onStop!),
                        icon: const Icon(Icons.stop),
                        label: Text(translate('Stop service')),
                      ),
                    ),
                  ),
                if (_mode == 'host' && widget.shareSettings != null)
                  Padding(
                    padding: const EdgeInsets.only(top: 16),
                    child: widget.shareSettings!,
                  ),
              ],
            ),
          ),
        ),
    );
    if (isAndroid) return page;
    return DropTarget(
      enable: _mode == 'control' &&
          !_busy &&
          widget.loadImageFile != null &&
          (ModalRoute.of(context)?.isCurrent ?? true),
      onDragDone: (details) => _act(() async {
        if (details.files.length != 1) {
          throw FormatException(translate('Drop one QR image at a time'));
        }
        final pairing = await widget.loadImageFile!(details.files.single.path);
        if (mounted) await _joinPairing(pairing);
      }),
      child: page,
    );
  }

  List<Widget> _relayWidgets() {
    final relay = _relay;
    if (relay == null || !relay.hasChip) return const [];
    final scheme = Theme.of(context).colorScheme;
    final Widget banner;
    switch (relay.state) {
      case 'connecting':
        final foreground = scheme.onSurface.withOpacity(0.6);
        banner = _relayChip(
          foreground: foreground,
          background: scheme.onSurface.withOpacity(0.08),
          icon: SizedBox(
            width: 12,
            height: 12,
            child: CircularProgressIndicator(
              strokeWidth: 2,
              color: foreground,
            ),
          ),
          label: translate('Checking relay...'),
        );
        break;
      case 'reachable':
        final rtt = relay.probeRttMs;
        final base = translate('Relay reachable');
        final foreground = scheme.primary;
        banner = _relayChip(
          foreground: foreground,
          background: foreground.withOpacity(0.08),
          icon: Icon(Icons.check_circle, size: 14, color: foreground),
          label: rtt != null && rtt > 0 ? '$base ($rtt ms)' : base,
        );
        break;
      case 'unreachable':
        final error = relay.lastError.trim();
        final foreground = scheme.error;
        banner = _relayChip(
          foreground: foreground,
          background: foreground.withOpacity(0.08),
          icon: Icon(Icons.warning_amber_rounded, size: 14, color: foreground),
          label: translate('Relay unreachable; scanning will likely time out'),
          detail: error.isEmpty ? null : error,
          tooltip: error.isEmpty ? null : error,
        );
        break;
      default:
        return const [];
    }
    return [
      const SizedBox(height: 16),
      banner,
    ];
  }

  Widget _relayChip({
    required Color foreground,
    required Color background,
    required Widget icon,
    required String label,
    String? detail,
    String? tooltip,
  }) {
    // Loose Flexible keeps a short label at its intrinsic width and ellipsizes
    // a long one instead of stretching the chip or overflowing the page.
    final chip = Align(
      alignment: Alignment.center,
      child: Container(
        key: const ValueKey('sealdesk-relay'),
        padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 4),
        decoration: BoxDecoration(
          color: background,
          borderRadius: BorderRadius.circular(16),
        ),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            icon,
            const SizedBox(width: 6),
            Flexible(
              child: Text(
                label,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(color: foreground, fontSize: 13),
              ),
            ),
          ],
        ),
      ),
    );
    final Widget body;
    if (detail == null || detail.isEmpty) {
      body = chip;
    } else {
      body = Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          chip,
          const SizedBox(height: 4),
          ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 280),
            child: Text(
              detail,
              key: const ValueKey('sealdesk-relay-error'),
              style: TextStyle(color: foreground, fontSize: 12),
              textAlign: TextAlign.center,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
            ),
          ),
        ],
      );
    }
    if (tooltip == null || tooltip.isEmpty) return body;
    return Tooltip(message: tooltip, child: body);
  }

  List<Widget> _buildHost() {
    switch (_phase) {
      case SealDeskSharePhase.stopped:
        return _buildStopped();
      case SealDeskSharePhase.permissionMissing:
        return [
          if (widget.permissionBanner != null) widget.permissionBanner!,
        ];
      case SealDeskSharePhase.starting:
        return _buildStarting();
      case SealDeskSharePhase.error:
        return _buildError();
      case SealDeskSharePhase.running:
        return _buildRunning();
    }
  }

  List<Widget> _buildStopped() => [
        Card(
          key: const ValueKey('sealdesk-share-stopped'),
          color: Theme.of(context).colorScheme.surfaceContainerHighest,
          child: Padding(
            padding: const EdgeInsets.all(16),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Row(children: [
                  const Icon(Icons.warning_amber_rounded,
                      color: Color(0xFFE37400)),
                  const SizedBox(width: 8),
                  Text(translate('Sharing stopped'),
                      key: const ValueKey('sealdesk-status'),
                      style: Theme.of(context).textTheme.titleMedium),
                ]),
                const SizedBox(height: 16),
                FilledButton.icon(
                  key: const ValueKey('sealdesk-start-service'),
                  onPressed: _busy || widget.onStart == null
                      ? null
                      : () => _act(widget.onStart!),
                  icon: const Icon(Icons.play_arrow),
                  label: Text(translate('Start service')),
                ),
              ],
            ),
          ),
        ),
        const SizedBox(height: 12),
        Row(
          key: const ValueKey('sealdesk-scam-line'),
          children: [
            const Icon(Icons.verified_user, size: 18, color: Color(0xFF8A5300)),
            const SizedBox(width: 6),
            Expanded(
              child: Text(
                translate('Stranger asks you to share? Hang up'),
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: const TextStyle(color: Color(0xFF8A5300), fontSize: 12),
              ),
            ),
          ],
        ),
      ];

  List<Widget> _buildStarting() => [
        Row(children: [
          const Icon(Icons.hourglass_top, size: 18),
          const SizedBox(width: 8),
          Expanded(
            child: Text(translate(_status),
                key: const ValueKey('sealdesk-status')),
          ),
          if (_resetButton() != null) _resetButton()!,
        ]),
        if (widget.onStop != null)
          Align(
            alignment: Alignment.centerLeft,
            child: TextButton(
              key: const ValueKey('sealdesk-stop-service'),
              onPressed: _busy ? null : () => _act(widget.onStop!),
              child: Text(translate('Stop')),
            ),
          ),
      ];

  List<Widget> _buildError() => [
        Text(_hostError ?? translate(_status),
            key: const ValueKey('sealdesk-status'),
            style: TextStyle(color: Theme.of(context).colorScheme.error)),
        TextButton.icon(
          key: const ValueKey('sealdesk-retry-host'),
          icon: const Icon(Icons.refresh),
          label: Text(translate('Retry')),
          onPressed: _busy ? null : _retryHost,
        ),
      ];

  Widget? _resetButton() {
    if (widget.resetAddress == null) return null;
    return TextButton.icon(
      key: const ValueKey('sealdesk-reset-address'),
      icon: const Icon(Icons.restart_alt),
      label: Text(translate('Reset address')),
      onPressed: _busy || (_pairing == null && _revokedAddress == null)
          ? null
          : _resetAddress,
    );
  }

  String? _ttlLabel() {
    final expires = _pairing?.expiresAt;
    if (expires == null) return null;
    final left = (expires - _now()).clamp(0, 24 * 60 * 60);
    final minutes = left ~/ 60;
    final seconds = (left % 60).toString().padLeft(2, '0');
    return '$minutes:$seconds';
  }

  List<Widget> _buildRunning() {
    final ttl = _ttlLabel();
    return [
        Wrap(
          spacing: 8,
          runSpacing: 8,
          crossAxisAlignment: WrapCrossAlignment.center,
          children: [
            Container(
              key: const ValueKey('sealdesk-sharing'),
              padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 4),
              decoration: BoxDecoration(
                color: const Color(0xFFE6F4EA),
                borderRadius: BorderRadius.circular(16),
              ),
              child: Text(translate('Sharing'),
                  style: const TextStyle(color: Color(0xFF1E7E34), fontSize: 13)),
            ),
            if (ttl != null)
              Container(
                key: const ValueKey('sealdesk-qr-ttl'),
                padding:
                    const EdgeInsets.symmetric(horizontal: 10, vertical: 4),
                decoration: BoxDecoration(
                  color: Theme.of(context)
                      .colorScheme
                      .onSurface
                      .withOpacity(0.08),
                  borderRadius: BorderRadius.circular(16),
                ),
                child: Text(ttl, style: const TextStyle(fontSize: 13)),
              ),
          ],
        ),
        ..._relayWidgets(),
        const SizedBox(height: 12),
        if (_code != null)
          Container(
            color: Colors.white,
            child: _hidden
                ? SizedBox(
                    width: 280,
                    height: 280,
                    child: Center(
                      child: Text(translate('QR code hidden'),
                          style: const TextStyle(color: Colors.black)),
                    ),
                  )
                : _buildQr(),
          ),
        const SizedBox(height: 12),
        Wrap(
          spacing: 8,
          runSpacing: 8,
          alignment: WrapAlignment.center,
          children: [
            TextButton.icon(
              key: const ValueKey('sealdesk-copy'),
              icon: const Icon(Icons.copy),
              label: Text(translate('Copy pairing code')),
              onPressed: _code == null || _busy
                  ? null
                  : () => _act(() => Clipboard.setData(
                      ClipboardData(text: _pairing!.encode(now: _now())))),
            ),
            TextButton.icon(
              icon: const Icon(Icons.refresh),
              label: Text(translate('Refresh password')),
              onPressed: _busy
                  ? null
                  : () => _act(() async {
                        await widget.refreshPassword();
                        await _poll(force: true);
                      }),
            ),
            if (_resetButton() != null) _resetButton()!,
            TextButton.icon(
              icon: Icon(_hidden ? Icons.visibility : Icons.visibility_off),
              label: Text(translate(_hidden ? 'Show QR code' : 'Hide QR code')),
              onPressed: () => setState(() => _hidden = !_hidden),
            ),
          ],
        ),
      ];
  }

  List<Widget> _buildControl() => [
        TextField(
          key: const ValueKey('sealdesk-input'),
          controller: _input,
          minLines: 3,
          maxLines: 6,
          autocorrect: false,
          enableSuggestions: false,
          decoration: InputDecoration(
            labelText: translate('Pairing code'),
            hintText: translate('Paste a pairing code starting with SEALDESK:'),
            border: const OutlineInputBorder(),
          ),
        ),
        const SizedBox(height: 16),
        Wrap(
          spacing: 12,
          children: [
            if (widget.pickImage != null)
              TextButton.icon(
                icon: const Icon(Icons.image),
                label: Text(translate('Load QR image')),
                onPressed: _busy ? null : _pickImage,
              ),
            TextButton.icon(
              icon: const Icon(Icons.paste),
              label: Text(translate('Paste text')),
              onPressed: _busy
                  ? null
                  : () => _act(() async {
                        final data =
                            await Clipboard.getData(Clipboard.kTextPlain);
                        if (data?.text == null || data!.text!.isEmpty) {
                          throw FormatException(translate(
                              'No pairing code text in the clipboard'));
                        }
                        final pairing =
                            SealDeskPairing.parse(data.text!, now: _now());
                        if (mounted) await _joinPairing(pairing);
                      }),
            ),
            if (widget.pasteImage != null)
              TextButton.icon(
                key: const ValueKey('sealdesk-paste-image'),
                icon: const Icon(Icons.content_paste),
                label: Text(translate('Paste QR image')),
                onPressed: _busy
                    ? null
                    : () => _act(() async {
                          final pairing = await widget.pasteImage!();
                          if (mounted) await _joinPairing(pairing);
                        }),
              ),
            FilledButton.icon(
              icon: const Icon(Icons.link),
              label: Text(translate(_busy ? 'Connecting...' : 'Connect')),
              onPressed: _busy ? null : _connect,
            ),
          ],
        ),
        if (widget.loadImageFile != null)
          Padding(
            padding: const EdgeInsets.only(top: 12),
            child: Text(translate(
                'You can also drop a QR screenshot here to connect automatically')),
          ),
      ];
}

class SealDeskRelaySettingsDialog extends StatefulWidget {
  final String initialUrl;
  final bool hosting;

  const SealDeskRelaySettingsDialog({
    super.key,
    required this.initialUrl,
    required this.hosting,
  });

  @override
  State<SealDeskRelaySettingsDialog> createState() =>
      _SealDeskRelaySettingsDialogState();
}

class _SealDeskRelaySettingsDialogState
    extends State<SealDeskRelaySettingsDialog> {
  late final _url = TextEditingController(
      text: widget.initialUrl.isEmpty ? _defaultDerpMapUrl : widget.initialUrl);
  String? _error;

  @override
  void dispose() {
    _url.dispose();
    super.dispose();
  }

  void _save() {
    final value = _url.text.trim();
    final uri = Uri.tryParse(value);
    if (value.isNotEmpty &&
        (uri == null ||
            uri.scheme != 'https' ||
            uri.host.isEmpty ||
            uri.authority.contains('@') ||
            uri.hasFragment ||
            RegExp(r'\s').hasMatch(value))) {
      setState(() => _error = translate(
          'Enter an HTTPS URL without credentials or a fragment'));
      return;
    }
    Navigator.pop(context, value == _defaultDerpMapUrl ? '' : value);
  }

  @override
  Widget build(BuildContext context) => AlertDialog(
        title: Text(translate('Relay settings')),
        content: SizedBox(
          width: 440,
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              TextField(
                key: const ValueKey('sealdesk-derp-map-url'),
                controller: _url,
                autocorrect: false,
                enableSuggestions: false,
                keyboardType: TextInputType.url,
                decoration: InputDecoration(
                  labelText: translate('DERP map URL'),
                  errorText: _error,
                  errorMaxLines: 3,
                  border: const OutlineInputBorder(),
                ),
                onSubmitted: (_) => _save(),
              ),
              const SizedBox(height: 16),
              if (widget.hosting)
                Text(translate(
                    'Saving restarts sharing and disconnects current sessions.')),
              Text(translate(
                  'Applies after sharing restarts or a new tunnel opens. Embedded relay addresses keep their relay.')),
            ],
          ),
        ),
        actions: [
          TextButton(
            onPressed: () => setState(() {
              _url.text = _defaultDerpMapUrl;
              _error = null;
            }),
            child: Text(translate('Reset to default')),
          ),
          TextButton(
            onPressed: () => Navigator.pop(context),
            child: Text(translate('Cancel')),
          ),
          FilledButton(
            onPressed: _save,
            child: Text(translate('Save')),
          ),
        ],
      );
}

Future<void> saveSealDeskDerpMapUrl(String value) async {
  Future<void> applyMode(String mode) async {
    await bind.mainSetLocalOption(key: 'sealdesk-mode', value: mode);
    final applied = await bind.mainGetOption(key: 'sealdesk-host-mode');
    if (applied != mode ||
        bind.mainGetLocalOption(key: 'sealdesk-mode') != mode) {
      throw FormatException(translate('Switch failed, please try again'));
    }
  }

  final hosting = bind.mainGetLocalOption(key: 'sealdesk-mode') == 'host';
  if (hosting) await applyMode('control');
  try {
    await bind.mainSetOption(key: 'tailcat-derp-map-url', value: value);
    if (await bind.mainGetOption(key: 'tailcat-derp-map-url') != value) {
      throw FormatException(
          translate('Unable to save relay settings, please try again'));
    }
  } finally {
    if (hosting) await applyMode('host');
  }
}

Future<void> showSealDeskRelaySettings(BuildContext context) async {
  if (bind.isDisableSettings()) return;
  final current = await bind.mainGetOption(key: 'tailcat-derp-map-url');
  if (!context.mounted) return;
  final hosting = bind.mainGetLocalOption(key: 'sealdesk-mode') == 'host';
  final value = await showDialog<String>(
    context: context,
    builder: (context) => SealDeskRelaySettingsDialog(
      initialUrl: current,
      hosting: hosting,
    ),
  );
  if (value == null || !context.mounted) return;
  try {
    await saveSealDeskDerpMapUrl(value);
  } on FormatException catch (error) {
    if (!context.mounted) return;
    await showDialog<void>(
      context: context,
      builder: (context) => AlertDialog(
        content: Text(error.message),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context),
            child: Text(translate('OK')),
          ),
        ],
      ),
    );
  }
}
