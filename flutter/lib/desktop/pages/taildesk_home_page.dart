import 'dart:async';

import 'package:desktop_drop/desktop_drop.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:qr_flutter/qr_flutter.dart';

import '../../common/taildesk_pairing.dart';

class TailDeskHostState {
  final String address;
  final String? password;
  final String name;
  final List<String> connectedNames;
  final bool stopped;

  const TailDeskHostState({
    required this.address,
    required this.password,
    required this.name,
    this.connectedNames = const [],
    this.stopped = false,
  });
}

class TailDeskHomePage extends StatefulWidget {
  final String initialMode;
  final String? initialInput;
  final Future<TailDeskHostState> Function() loadHost;
  final Future<void> Function() refreshPassword;
  final Future<void> Function(TailDeskPairing) connect;
  final Future<void> Function(String) saveMode;
  final int Function()? now;
  final Future<TailDeskPairing?> Function()? pickImage;
  final Future<TailDeskPairing> Function(String)? loadImageFile;
  final Future<void> Function()? resetAddress;
  final Future<TailDeskPairing> Function()? pasteImage;

  const TailDeskHomePage({
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
  State<TailDeskHomePage> createState() => _TailDeskHomePageState();
}

class _TailDeskHomePageState extends State<TailDeskHomePage> {
  final _input = TextEditingController();
  Timer? _timer;
  late String _mode;
  TailDeskPairing? _pairing;
  String? _code;
  String? _revokedAddress;
  String _status = '正在准备安全连接…';
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
        final TailDeskPairing pairing;
        if (TailDeskPairing.isPairingCode(input)) {
          pairing = TailDeskPairing.parse(input, now: _now());
        } else {
          final loader = widget.loadImageFile;
          if (loader == null) {
            throw const FormatException('当前版本无法导入二维码图片');
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
          _status = '正在等待新的安全连接地址…';
        });
        return;
      }
      if (host.stopped || !TailDeskPairing.isAddress(host.address)) {
        setState(() {
          _pairing = null;
          _code = null;
          _status = host.stopped ? '被控服务已停止，请启动服务' : '正在准备安全连接…';
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
        _pairing = TailDeskPairing(
          address: host.address,
          password: host.password,
          name: host.name,
          expiresAt: now + 600,
        );
        _code = _pairing!.encode(now: now);
      }
      setState(() {
        _status = host.connectedNames.isEmpty
            ? '等待连接'
            : '已连接：${host.connectedNames.join('、')}';
      });
    } catch (_) {
      if (mounted && _mode == 'host') {
        setState(() {
          _pairing = null;
          _code = null;
          _status = '暂时无法读取被控服务，请确认服务已启动';
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
        _status = '正在准备安全连接…';
      });
      await _poll(force: true);
    });
  }

  Future<void> _resetAddress() => _act(() async {
        final confirmed = await showDialog<bool>(
          context: context,
          builder: (context) => AlertDialog(
            title: const Text('重置被控地址？'),
            content: const Text('这会断开当前被控会话，并使所有旧二维码失效。请将新二维码重新分享给对方。'),
            actions: [
              TextButton(
                onPressed: () => Navigator.pop(context, false),
                child: const Text('取消'),
              ),
              FilledButton(
                onPressed: () => Navigator.pop(context, true),
                child: const Text('确认重置'),
              ),
            ],
          ),
        );
        if (confirmed != true || !mounted) return;
        setState(() {
          _revokedAddress = _pairing?.address ?? _revokedAddress;
          _pairing = null;
          _code = null;
          _status = '正在重置地址…';
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
        final pairing = TailDeskPairing.parse(_input.text, now: _now());
        await _joinPairing(pairing);
      });

  Future<void> _joinPairing(TailDeskPairing pairing) async {
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
      return const Text('配对码过长，无法生成二维码，请复制配对码文本');
    }
    final quietZone = (280 * 4 / (qr.moduleCount + 8)).ceilToDouble();
    return QrImageView.withQr(
      key: const ValueKey('taildesk-qr'),
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
          throw const FormatException('请一次拖入一张二维码图片');
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
                Text('TailDesk',
                    style: Theme.of(context).textTheme.headlineMedium),
                const SizedBox(height: 12),
                const Text('扫码即连，无需账号'),
                const SizedBox(height: 24),
                Wrap(
                  spacing: 12,
                  runSpacing: 12,
                  alignment: WrapAlignment.center,
                  children: [
                    ChoiceChip(
                      label: const Text('被控此设备'),
                      selected: _mode == 'host',
                      onSelected: _busy ? null : (_) => _selectMode('host'),
                    ),
                    ChoiceChip(
                      label: const Text('控制其他设备'),
                      selected: _mode == 'control',
                      onSelected: _busy ? null : (_) => _selectMode('control'),
                    ),
                  ],
                ),
                const SizedBox(height: 24),
                if (_mode.isEmpty) const Text('请选择使用方式，下次启动会记住你的选择'),
                if (_mode == 'host') ...[
                  ..._buildHost(),
                  const SizedBox(height: 12),
                  const Text('切换为控制模式会断开当前被控会话'),
                ],
                if (_mode == 'control') ..._buildControl(),
                if (_error != null)
                  Padding(
                    padding: const EdgeInsets.only(top: 16),
                    child: Text(_error!,
                        key: const ValueKey('taildesk-error'),
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
        Text(_status, key: const ValueKey('taildesk-status')),
        const SizedBox(height: 16),
        if (_code != null)
          Container(
            color: Colors.white,
            child: _hidden
                ? const SizedBox(
                    width: 280,
                    height: 280,
                    child: Center(
                      child:
                          Text('二维码已遮挡', style: TextStyle(color: Colors.black)),
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
              key: const ValueKey('taildesk-copy'),
              icon: const Icon(Icons.copy),
              label: const Text('复制配对码'),
              onPressed: _code == null || _busy
                  ? null
                  : () => _act(() => Clipboard.setData(
                      ClipboardData(text: _pairing!.encode(now: _now())))),
            ),
            TextButton.icon(
              icon: const Icon(Icons.refresh),
              label: const Text('刷新密码'),
              onPressed: _busy
                  ? null
                  : () => _act(() async {
                        await widget.refreshPassword();
                        await _poll(force: true);
                      }),
            ),
            if (widget.resetAddress != null)
              TextButton.icon(
                key: const ValueKey('taildesk-reset-address'),
                icon: const Icon(Icons.restart_alt),
                label: const Text('重置地址'),
                onPressed:
                    _busy || (_pairing == null && _revokedAddress == null)
                        ? null
                        : _resetAddress,
              ),
            TextButton.icon(
              icon: Icon(_hidden ? Icons.visibility : Icons.visibility_off),
              label: Text(_hidden ? '显示二维码' : '遮挡二维码'),
              onPressed: () => setState(() => _hidden = !_hidden),
            ),
          ],
        ),
        const SizedBox(height: 8),
        const Text('官方客户端将在 10 分钟后拒绝导入此码；撤销旧临时密码的新登录能力需刷新密码。请只分享给可信任的人。'),
      ];

  List<Widget> _buildControl() => [
        TextField(
          key: const ValueKey('taildesk-input'),
          controller: _input,
          minLines: 3,
          maxLines: 6,
          autocorrect: false,
          enableSuggestions: false,
          decoration: const InputDecoration(
            labelText: '配对码',
            hintText: '粘贴 TAILDESK: 开头的配对码',
            border: OutlineInputBorder(),
          ),
        ),
        const SizedBox(height: 16),
        Wrap(
          spacing: 12,
          children: [
            if (widget.pickImage != null)
              TextButton.icon(
                icon: const Icon(Icons.image),
                label: const Text('载入二维码图片'),
                onPressed: _busy ? null : _pickImage,
              ),
            TextButton.icon(
              icon: const Icon(Icons.paste),
              label: const Text('粘贴文本'),
              onPressed: _busy
                  ? null
                  : () => _act(() async {
                        final data =
                            await Clipboard.getData(Clipboard.kTextPlain);
                        if (data?.text == null || data!.text!.isEmpty) {
                          throw const FormatException('剪贴板中没有配对码文本');
                        }
                        final pairing =
                            TailDeskPairing.parse(data.text!, now: _now());
                        if (mounted) await _joinPairing(pairing);
                      }),
            ),
            if (widget.pasteImage != null)
              TextButton.icon(
                key: const ValueKey('taildesk-paste-image'),
                icon: const Icon(Icons.content_paste),
                label: const Text('粘贴二维码图片'),
                onPressed: _busy
                    ? null
                    : () => _act(() async {
                          final pairing = await widget.pasteImage!();
                          if (mounted) await _joinPairing(pairing);
                        }),
              ),
            FilledButton.icon(
              icon: const Icon(Icons.link),
              label: Text(_busy ? '正在连接…' : '连接'),
              onPressed: _busy ? null : _connect,
            ),
          ],
        ),
        if (widget.loadImageFile != null)
          const Padding(
            padding: EdgeInsets.only(top: 12),
            child: Text('也可以将一张二维码截图拖入此页面，识别后自动连接'),
          ),
      ];
}
