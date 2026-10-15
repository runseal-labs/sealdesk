import 'dart:async';

import 'package:flutter/material.dart';
import 'package:qr_code_scanner/qr_code_scanner.dart';

import '../../common/taildesk_pairing.dart';

class TailDeskScanPage extends StatefulWidget {
  final Future<TailDeskPairing?> Function()? pickImage;
  final int Function()? now;

  const TailDeskScanPage({super.key, this.pickImage, this.now});

  @override
  State<TailDeskScanPage> createState() => _TailDeskScanPageState();
}

class _TailDeskScanPageState extends State<TailDeskScanPage>
    with WidgetsBindingObserver {
  final _qrKey = GlobalKey(debugLabel: 'taildesk-camera');
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
          setState(() => _error = '摄像头操作失败，请返回后重新打开扫码');
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
      if (mounted) setState(() => _error = '扫码失败，请返回后重新打开摄像头');
    });
    if (!_active) _syncCamera();
  }

  void _readCode(String code) {
    if (!mounted || !_active || _busy || _finished) return;
    try {
      final pairing = TailDeskPairing.parse(code, now: _now());
      _finish(pairing);
    } on FormatException catch (error) {
      if (_error != error.message) {
        setState(() => _error = error.message);
      }
    }
  }

  Future<void> _finish(TailDeskPairing pairing,
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
        throw const FormatException('扫描已暂停，请返回前台后重新扫描');
      }
      pairing.encode(now: _now());
      _finished = true;
      Navigator.of(context).pop(pairing);
    } on FormatException catch (error) {
      setState(() => _error = error.message);
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
      if (mounted) setState(() => _error = error.message);
    } catch (_) {
      if (mounted) setState(() => _error = '无法读取二维码图片，请重新选择');
    } finally {
      if (mounted && !_finished) {
        setState(() => _busy = false);
        await _syncCamera();
      }
    }
  }

  @override
  Widget build(BuildContext context) => Scaffold(
        appBar: AppBar(title: const Text('扫码连接 TailDesk')),
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
                  _error = granted ? null : '没有摄像头权限，请在系统设置中允许后重新打开扫码，或载入图片';
                });
                if (!granted) _syncCamera();
              },
            ),
          ),
          Padding(
            padding: const EdgeInsets.all(16),
            child: Text(_busy ? '正在识别并准备连接…' : '请将对方的 TailDesk 配对二维码放入框内'),
          ),
          if (_error != null)
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 16),
              child: Text(_error!, key: const ValueKey('taildesk-scan-error')),
            ),
          if (widget.pickImage != null)
            TextButton.icon(
              onPressed: _busy ? null : _pickImage,
              icon: const Icon(Icons.image),
              label: const Text('载入二维码图片'),
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
