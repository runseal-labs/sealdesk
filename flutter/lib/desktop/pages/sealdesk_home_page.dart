import 'dart:async';

import 'package:desktop_drop/desktop_drop.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:qr_flutter/qr_flutter.dart';

import '../../common.dart';
import '../../common/sealdesk_pairing.dart';

class SealDeskHostState {
  final String address;
  final String? password;
  final String name;
  final List<String> connectedNames;
  final bool stopped;

  const SealDeskHostState({
    required this.address,
    required this.password,
    required this.name,
    this.connectedNames = const [],
    this.stopped = false,
  });
}

class SealDeskHomePage extends StatefulWidget {
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

  const SealDeskHomePage({
    super.key,
    required this.initialMode,
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

  String get _statusText => translate(_status);
  String? _error;
  bool _hidden = false;
  bool _loading = false;
  bool _busy = false;

  int _now() =>
      widget.now?.call() ?? DateTime.now().millisecondsSinceEpoch ~/ 1000;

  @override
  void initState() {
    super.initState();
    _mode = const ['host', 'control'].contains(widget.initialMode)
        ? widget.initialMode
        : '';
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
      if (host.address == _revokedAddress) {
        setState(() {
          _pairing = null;
          _code = null;
          _status = 'Updating QR code...';
        });
        return;
      }
      if (host.stopped || !SealDeskPairing.isAddress(host.address)) {
        setState(() {
          _pairing = null;
          _code = null;
          _status = host.stopped
              ? translate('Sharing stopped')
              : translate('Generating pairing code...');
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
        _status = host.connectedNames.isEmpty
            ? translate('Waiting for connection')
            : '已连接：${host.connectedNames.join('、')}';
      });
    } catch (_) {
      if (mounted && _mode == 'host') {
        setState(() {
          _pairing = null;
          _code = null;
          _status = 'Unable to get pairing code, please try again';
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
        _pairing = null;
        _code = null;
        _status = 'Generating pairing code...';
      });
      await _poll(force: true);
    });
  }

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
          _revokedAddress = _pairing?.address ?? _revokedAddress;
          _pairing = null;
          _code = null;
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
      if (mounted) setState(() => _error = error.message);
    } catch (_) {
      if (mounted) setState(() => _error = '操作失败，请重试');
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
      child: Center(
        child: SingleChildScrollView(
          padding: const EdgeInsets.all(24),
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 520),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                Text('SealDesk',
                    style: Theme.of(context).textTheme.headlineMedium),
                const SizedBox(height: 12),
                Text(translate('Scan to connect, no account required')),
                const SizedBox(height: 24),
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
                      onSelected: _busy ? null : (_) => _selectMode('control'),
                    ),
                  ],
                ),
                const SizedBox(height: 24),
                if (_mode.isEmpty)
                  Text(translate(
                      'Choose how to use SealDesk. Your choice will be remembered.')),
                if (_mode == 'host') ...[
                  ..._buildHost(),
                  const SizedBox(height: 12),
                  Text(translate(
                      'Switching will disconnect the current session')),
                ],
                if (_mode == 'control') ..._buildControl(),
                if (_error != null)
                  Padding(
                    padding: const EdgeInsets.only(top: 16),
                    child: Text(_error!,
                        key: const ValueKey('sealdesk-error'),
                        style: TextStyle(
                            color: Theme.of(context).colorScheme.error)),
                  ),
              ],
            ),
          ),
        ),
      ),
    );
  }

  List<Widget> _buildHost() => [
        Text(_statusText, key: const ValueKey('sealdesk-status')),
        const SizedBox(height: 16),
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
        const SizedBox(height: 16),
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
            if (widget.resetAddress != null)
              TextButton.icon(
                key: const ValueKey('sealdesk-reset-address'),
                icon: const Icon(Icons.restart_alt),
                label: Text(translate('Reset address')),
                onPressed:
                    _busy || (_pairing == null && _revokedAddress == null)
                        ? null
                        : _resetAddress,
              ),
            TextButton.icon(
              icon: Icon(_hidden ? Icons.visibility : Icons.visibility_off),
              label: Text(translate(_hidden ? 'Show QR code' : 'Hide QR code')),
              onPressed: () => setState(() => _hidden = !_hidden),
            ),
          ],
        ),
        const SizedBox(height: 8),
        Text(translate(
            'QR codes are valid for 10 minutes. Refresh the password to invalidate old codes. Share carefully.')),
      ];

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
