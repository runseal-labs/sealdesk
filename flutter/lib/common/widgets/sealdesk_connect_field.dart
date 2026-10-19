import 'dart:convert';
import 'dart:typed_data';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:image_picker/image_picker.dart';
import 'package:window_manager/window_manager.dart';

import '../../common.dart';
import '../../models/platform_model.dart';
import '../sealdesk_connect.dart';
import '../sealdesk_connect_target.dart';
import '../sealdesk_connection_record.dart';
import '../sealdesk_pairing.dart';
import '../sealdesk_qr_file.dart';
import '../sealdesk_qr_image.dart';

Future<Uint8List> captureSealDeskRegion(
    Rect rect, double devicePixelRatio) async {
  var origin = Offset.zero;
  try {
    origin = await windowManager
        .getPosition()
        .timeout(const Duration(milliseconds: 500));
  } catch (_) {}
  final scale = devicePixelRatio <= 0 ? 1.0 : devicePixelRatio;
  final reply = jsonDecode(await bind.mainCaptureSealdeskRegion(
    x: ((origin.dx + rect.left) * scale).round(),
    y: ((origin.dy + rect.top) * scale).round(),
    width: (rect.width * scale).round(),
    height: (rect.height * scale).round(),
  )) as Map<String, dynamic>;
  final error = reply['error'];
  if (error is String) throw FormatException(error);
  final png = reply['png'];
  if (png is! String || png.isEmpty) {
    throw const FormatException(
        'Unable to read the QR image. Please choose another one');
  }
  return base64Decode(png);
}

class SealDeskConnectField extends StatefulWidget {
  final TextEditingController controller;
  final bool desktop;
  final Future<void> Function()? onScan;

  const SealDeskConnectField({
    super.key,
    required this.controller,
    this.desktop = false,
    this.onScan,
  });

  @override
  State<SealDeskConnectField> createState() => _SealDeskConnectFieldState();
}

class _SealDeskConnectFieldState extends State<SealDeskConnectField> {
  bool _busy = false;
  String? _error;

  Future<void> _run(Future<void> Function() action) async {
    if (_busy) return;
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      await action();
    } on FormatException catch (error) {
      final message = translate(error.message);
      if (mounted) {
        setState(() => _error = message);
        showToast(message);
      }
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _submit() => _run(() async {
        await connectSealDeskInput(context, widget.controller.text);
      });

  Future<void> _usePairing(SealDeskPairing pairing) =>
      connectSealDeskPairing(context, pairing);

  Future<void> _gallery() => _run(() async {
        final file =
            await ImagePicker().pickImage(source: ImageSource.gallery);
        if (file == null || !mounted) return;
        await _usePairing(await loadSealDeskQrFile(file.path));
      });

  Future<void> _file() => _run(() async {
        final result = await FilePicker.platform.pickFiles(
          type: FileType.custom,
          allowedExtensions: ['png', 'jpg', 'jpeg'],
        );
        if (result == null || !mounted) return;
        final path = result.files.single.path;
        if (path == null) {
          throw const FormatException(
              'Unable to read the selected image. Please choose again');
        }
        await _usePairing(await loadSealDeskQrFile(path));
      });

  Future<void> _screen() => _run(() async {
        try {
          await _usePairing(await pasteSealDeskClipboardImage());
          return;
        } on FormatException {
          if (!mounted) return;
        }
        final rect = await showDialog<Rect>(
          context: context,
          barrierColor: Colors.black54,
          builder: (context) => const SealDeskRegionPicker(),
        );
        if (rect == null || !mounted) return;
        final bytes = await captureSealDeskRegion(
            rect, MediaQuery.devicePixelRatioOf(context));
        if (!mounted) return;
        await _usePairing(decodeSealDeskQrImage(bytes));
      });

  @override
  Widget build(BuildContext context) {
    final icons = widget.desktop
        ? <Widget>[
            _icon(
              key: const ValueKey('sealdesk-connect-screen'),
              tooltip: kSealDeskScreenTooltip,
              icon: const Icon(Icons.screenshot_monitor),
              onPressed: _busy ? null : _screen,
            ),
            _icon(
              key: const ValueKey('sealdesk-connect-file'),
              tooltip: kSealDeskImageTooltip,
              icon: const Icon(Icons.image_search),
              onPressed: _busy ? null : _file,
            ),
          ]
        : <Widget>[
            _icon(
              key: const ValueKey('sealdesk-connect-scan'),
              tooltip: kSealDeskScanTooltip,
              icon: const SealDeskScanIcon(),
              onPressed: _busy || widget.onScan == null
                  ? null
                  : () => _run(() async {
                        await widget.onScan!();
                      }),
            ),
            _icon(
              key: const ValueKey('sealdesk-connect-gallery'),
              tooltip: kSealDeskGalleryTooltip,
              icon: const Icon(Icons.image_search),
              onPressed: _busy ? null : _gallery,
            ),
          ];
    return Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        DecoratedBox(
          decoration: BoxDecoration(
            color: Theme.of(context).colorScheme.surfaceContainerHighest,
            borderRadius: BorderRadius.circular(12),
          ),
          child: Row(
            children: [
              Expanded(
                child: TextField(
                  key: const ValueKey('sealdesk-connect-input'),
                  controller: widget.controller,
                  autocorrect: false,
                  enableSuggestions: false,
                  keyboardType: TextInputType.visiblePassword,
                  minLines: 1,
                  maxLines: 1,
                  style: const TextStyle(fontSize: 16),
                  decoration: InputDecoration(
                    hintText: translate(kSealDeskHint),
                    hintMaxLines: 1,
                    isDense: true,
                    contentPadding:
                        const EdgeInsets.symmetric(horizontal: 12, vertical: 14),
                    border: InputBorder.none,
                  ),
                  onSubmitted: (_) => _submit(),
                ),
              ),
              ...icons,
              _icon(
                key: const ValueKey('sealdesk-connect-go'),
                tooltip: 'Connect',
                icon: const Icon(Icons.arrow_forward),
                onPressed: _busy ? null : _submit,
              ),
            ],
          ),
        ),
        if (_error != null)
          Padding(
            padding: const EdgeInsets.only(top: 6, left: 4),
            child: Text(
              _error!,
              key: const ValueKey('sealdesk-connect-error'),
              maxLines: 2,
              overflow: TextOverflow.ellipsis,
              style: TextStyle(color: Theme.of(context).colorScheme.error),
            ),
          ),
      ],
    );
  }

  Widget _icon({
    required Key key,
    required String tooltip,
    required Widget icon,
    required VoidCallback? onPressed,
  }) {
    return IconButton(
      key: key,
      tooltip: translate(tooltip),
      visualDensity: VisualDensity.compact,
      constraints: const BoxConstraints.tightFor(width: 40, height: 40),
      padding: EdgeInsets.zero,
      onPressed: onPressed,
      icon: icon,
    );
  }
}

class SealDeskScanIcon extends StatelessWidget {
  final double size;
  const SealDeskScanIcon({super.key, this.size = 22});

  @override
  Widget build(BuildContext context) {
    return CustomPaint(
      size: Size.square(size),
      painter: _ScanPainter(
          color: IconTheme.of(context).color ?? const Color(0xFF444444)),
    );
  }
}

class _ScanPainter extends CustomPainter {
  final Color color;
  _ScanPainter({required this.color});

  @override
  void paint(Canvas canvas, Size size) {
    final paint = Paint()
      ..color = color
      ..style = PaintingStyle.stroke
      ..strokeWidth = 1.8
      ..strokeCap = StrokeCap.square;
    final len = size.width * 0.28;
    final inset = size.width * 0.08;
    final right = size.width - inset;
    final bottom = size.height - inset;
    void corner(Offset a, Offset b, Offset c) {
      canvas.drawLine(a, b, paint);
      canvas.drawLine(b, c, paint);
    }

    corner(Offset(inset, inset + len), Offset(inset, inset),
        Offset(inset + len, inset));
    corner(Offset(right - len, inset), Offset(right, inset),
        Offset(right, inset + len));
    corner(Offset(inset, bottom - len), Offset(inset, bottom),
        Offset(inset + len, bottom));
    corner(Offset(right - len, bottom), Offset(right, bottom),
        Offset(right, bottom - len));
    final y = size.height * 0.52;
    canvas.drawLine(Offset(inset + 2, y), Offset(right - 2, y), paint);
  }

  @override
  bool shouldRepaint(covariant _ScanPainter oldDelegate) =>
      oldDelegate.color != color;
}

class SealDeskRegionPicker extends StatefulWidget {
  const SealDeskRegionPicker({super.key});

  @override
  State<SealDeskRegionPicker> createState() => _SealDeskRegionPickerState();
}

class _SealDeskRegionPickerState extends State<SealDeskRegionPicker> {
  Offset? _start;
  Offset? _end;

  @override
  Widget build(BuildContext context) {
    return Material(
      key: const ValueKey('sealdesk-region-picker'),
      type: MaterialType.transparency,
      child: SizedBox.expand(
        child: GestureDetector(
          behavior: HitTestBehavior.opaque,
          onPanStart: (details) => setState(() {
            _start = details.localPosition;
            _end = details.localPosition;
          }),
          onPanUpdate: (details) =>
              setState(() => _end = details.localPosition),
          onPanEnd: (_) {
            final start = _start;
            final end = _end;
            if (start == null || end == null) return;
            final rect = Rect.fromPoints(start, end);
            if (rect.width < 8 || rect.height < 8) return;
            final box = context.findRenderObject() as RenderBox?;
            final global = box == null
                ? rect
                : Rect.fromPoints(
                    box.localToGlobal(rect.topLeft),
                    box.localToGlobal(rect.bottomRight),
                  );
            Navigator.pop(context, global);
          },
          child: CustomPaint(
            painter: _RegionPainter(start: _start, end: _end),
            child: Align(
              alignment: Alignment.topRight,
              child: IconButton(
                tooltip: translate('Cancel'),
                onPressed: () => Navigator.pop(context),
                icon: const Icon(Icons.close, color: Colors.white),
              ),
            ),
          ),
        ),
      ),
    );
  }
}

class _RegionPainter extends CustomPainter {
  final Offset? start;
  final Offset? end;
  _RegionPainter({this.start, this.end});

  @override
  void paint(Canvas canvas, Size size) {
    final start = this.start;
    final end = this.end;
    if (start == null || end == null) return;
    final rect = Rect.fromPoints(start, end);
    canvas.drawRect(
      rect,
      Paint()
        ..style = PaintingStyle.stroke
        ..strokeWidth = 2
        ..color = const Color(0xFF8FD3FF),
    );
  }

  @override
  bool shouldRepaint(covariant _RegionPainter oldDelegate) =>
      oldDelegate.start != start || oldDelegate.end != end;
}

class SealDeskSecurityChip extends StatelessWidget {
  final bool encrypted;
  const SealDeskSecurityChip({super.key, required this.encrypted});

  @override
  Widget build(BuildContext context) {
    final color = encrypted ? const Color(0xFF1B8A3A) : const Color(0xFF6B7280);
    return Container(
      key: ValueKey(encrypted ? 'sealdesk-chip-encrypted' : 'sealdesk-chip-compat'),
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 2),
      decoration: BoxDecoration(
        color: color.withOpacity(0.12),
        borderRadius: BorderRadius.circular(12),
      ),
      child: Text(
        translate(encrypted ? kSealDeskEncryptedChip : kSealDeskCompatChip),
        maxLines: 1,
        overflow: TextOverflow.ellipsis,
        style: TextStyle(color: color, fontSize: 12),
      ),
    );
  }
}

class SealDeskLanMark extends StatelessWidget {
  const SealDeskLanMark({super.key});

  @override
  Widget build(BuildContext context) {
    return Container(
      key: const ValueKey('sealdesk-lan-mark'),
      margin: const EdgeInsets.only(left: 6),
      padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 1),
      decoration: BoxDecoration(
        color: Theme.of(context).colorScheme.surfaceContainerHighest,
        borderRadius: BorderRadius.circular(8),
      ),
      child: const Text(
        sealDeskLanBadge,
        style: TextStyle(fontSize: 11),
      ),
    );
  }
}
