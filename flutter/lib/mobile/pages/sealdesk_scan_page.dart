import 'dart:async';

import 'package:flutter/material.dart';

import '../../common.dart';
import 'package:qr_code_scanner/qr_code_scanner.dart';

import '../../common/sealdesk_pairing.dart';

class SealDeskScanPage extends StatefulWidget {
  final Future<SealDeskPairing?> Function()? pickImage;
  final int Function()? now;

  const SealDeskScanPage({super.key, this.pickImage, this.now});

  @override
  State<SealDeskScanPage> createState() => _SealDeskScanPageState();
}

class _SealDeskScanPageState extends State<SealDeskScanPage>
    with WidgetsBindingObserver {
  final _qrKey = GlobalKey(debugLabel: 'sealdesk-camera');
  QRViewController? _controller;
  StreamSubscription<Barcode>? _subscription;
  Future<bool> _cameraWork = Future.value(true);
  bool _active = true;
  bool _busy = false;
  bool _finished = false;
  bool _permissionDenied = false;
  String? _error;

  int _now() =>
      widget.now?.call() ?? DateTime.now().millisecondsSinceEpoch ~/ 1000;

  @override
  void initState() {
    super.initState();
    final lifecycle = WidgetsBinding.instance.lifecycleState;
    _active = lifecycle == null || lifecycle == AppLifecycleState.resumed;
    WidgetsBinding.instance.addObserver(this);
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    _active = state == AppLifecycleState.resumed;
    _syncCamera();
  }

  Future<bool> _syncCamera() {
    final controller = _controller;
    return _cameraWork = _cameraWork.then((_) async {
      if (!mounted || controller != _controller) {
        return false;
      }
      if (controller == null) return true;
      try {
        if (_active && !_busy && !_finished && !_permissionDenied) {
          await controller.resumeCamera();
        } else {
          await controller.pauseCamera();
        }
        return true;
      } catch (_) {
        if (mounted) {
          setState(() => _error = translate(
              'Camera operation failed. Go back and reopen the scanner'));
        }
        return false;
      }
    });
  }

  void _onCreated(QRViewController controller) {
    if (!mounted) {
      controller.dispose();
      return;
    }
    _controller = controller;
    _subscription = controller.scannedDataStream.listen((barcode) {
      if (barcode.code != null && !_permissionDenied) {
        _readCode(barcode.code!);
      }
    }, onError: (_) {
      if (mounted) {
        setState(() =>
            _error = translate('Scan failed. Go back and reopen the camera'));
      }
    });
    if (!_active) _syncCamera();
  }

  void _readCode(String code) {
    if (!mounted || !_active || _busy || _finished) return;
    try {
      final pairing = SealDeskPairing.parse(code, now: _now());
      _finish(pairing);
    } on FormatException catch (error) {
      final message = translate(error.message);
      if (_error != message) {
        setState(() => _error = message);
      }
    }
  }

  Future<void> _finish(SealDeskPairing pairing,
      {bool fromImage = false}) async {
    if (!mounted || _finished) return;
    setState(() {
      _busy = true;
      _error = null;
    });
    final paused = await _syncCamera();
    if (!mounted) return;
    try {
      if (!paused) return;
      if (!_active || (!fromImage && _permissionDenied)) {
        throw FormatException(
            translate('Scanning is paused. Return to the app and scan again'));
      }
      pairing.encode(now: _now());
      _finished = true;
      Navigator.of(context).pop(pairing);
    } on FormatException catch (error) {
      setState(() => _error = translate(error.message));
    } finally {
      if (mounted && !_finished) {
        setState(() => _busy = false);
        await _syncCamera();
      }
    }
  }

  Future<void> _pickImage() async {
    if (_busy || _finished) return;
    setState(() {
      _busy = true;
      _error = null;
    });
    await _syncCamera();
    try {
      final pairing = await widget.pickImage!();
      if (mounted && pairing != null) await _finish(pairing, fromImage: true);
    } on FormatException catch (error) {
      if (mounted) setState(() => _error = translate(error.message));
    } catch (_) {
      if (mounted)
        setState(() => _error = translate(
            'Unable to read the QR image. Please choose another one'));
    } finally {
      if (mounted && !_finished) {
        setState(() => _busy = false);
        await _syncCamera();
      }
    }
  }

  @override
  Widget build(BuildContext context) => Scaffold(
        appBar: AppBar(title: Text(translate('Scan to connect to SealDesk'))),
        body: Column(children: [
          Expanded(
            child: QRView(
              key: _qrKey,
              onQRViewCreated: _onCreated,
              formatsAllowed: const [BarcodeFormat.qrcode],
              overlay: QrScannerOverlayShape(
                borderColor: Colors.white,
                borderWidth: 4,
                cutOutSize: 240,
              ),
              onPermissionSet: (_, granted) {
                if (!mounted) return;
                setState(() {
                  _permissionDenied = !granted;
                  _error = granted
                      ? null
                      : translate(
                          'Camera permission is required. Allow it in system settings, then reopen the scanner, or load an image');
                });
                if (!granted) _syncCamera();
              },
            ),
          ),
          Padding(
            padding: const EdgeInsets.all(16),
            child: Text(_busy
                ? translate('Reading and preparing the connection...')
                : translate(
                    'Place the other device’s SealDesk QR code in the frame')),
          ),
          if (_error != null)
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 16),
              child: Text(_error!, key: const ValueKey('sealdesk-scan-error')),
            ),
          if (widget.pickImage != null)
            TextButton.icon(
              onPressed: _busy ? null : _pickImage,
              icon: const Icon(Icons.image),
              label: Text(translate('Load QR image')),
            ),
          const SafeArea(top: false, child: SizedBox(height: 16)),
        ]),
      );

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    _subscription?.cancel();
    _controller?.dispose();
    _controller = null;
    super.dispose();
  }
}
