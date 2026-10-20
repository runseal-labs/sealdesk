import 'dart:io';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sealdesk/mobile/widgets/terminal_quick_key_bar.dart';
import 'package:xterm/xterm.dart';

const _shotFont = 'DejaVuSans';
const _shotMonoFont = 'DejaVuSansMono';

const _sansFontPaths = <String>[
  '/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf',
  '/usr/share/fonts/dejavu/DejaVuSans.ttf',
  '/usr/share/fonts/TTF/DejaVuSans.ttf',
  '/Library/Fonts/Arial.ttf',
  '/System/Library/Fonts/Supplemental/Arial.ttf',
  r'C:\Windows\Fonts\arial.ttf',
];

const _monoFontPaths = <String>[
  '/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf',
  '/usr/share/fonts/dejavu/DejaVuSansMono.ttf',
  '/usr/share/fonts/TTF/DejaVuSansMono.ttf',
  '/Library/Fonts/Courier New.ttf',
  '/System/Library/Fonts/Supplemental/Courier New.ttf',
  r'C:\Windows\Fonts\cour.ttf',
];

/// Loads the first font file that exists. Missing platform fonts are skipped
/// so `flutter test` can run without a Linux font directory.
Future<String?> _loadFirstFont(String family, List<String> paths) async {
  for (final path in paths) {
    final file = File(path);
    if (!file.existsSync()) continue;
    final bytes = file.readAsBytesSync();
    final loader = FontLoader(family)
      ..addFont(Future.value(ByteData.sublistView(bytes)));
    await loader.load();
    return family;
  }
  return null;
}

void main() {
  testWidgets('renders the terminal page with quick keys at phone sizes',
      (tester) async {
    const cases = <(String, Size)>[
      ('terminal-keys-portrait', Size(390, 844)),
      ('terminal-keys-landscape', Size(844, 390)),
    ];
    for (final item in cases) {
      final name = item.$1;
      final size = item.$2;
      tester.view.devicePixelRatio = 1;
      tester.view.physicalSize = size;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);

      final sansFont = await _loadFirstFont(_shotFont, _sansFontPaths);
      final monoFont = await _loadFirstFont(_shotMonoFont, _monoFontPaths);
      final terminal = Terminal()..write('SealDesk\r\n\$ ');
      final controller = TerminalController();
      addTearDown(controller.dispose);
      await tester.pumpWidget(MaterialApp(
        debugShowCheckedModeBanner: false,
        theme: ThemeData(
          useMaterial3: true,
          brightness: Brightness.dark,
          fontFamily: sansFont,
          scaffoldBackgroundColor: const Color(0xFF101114),
        ),
        home: MediaQuery(
          data: MediaQueryData(size: size),
          child: Scaffold(
            body: SizedBox(
              width: size.width,
              height: size.height,
              child: RepaintBoundary(
                key: const Key('terminal-keys-shot'),
                child: Column(
                  children: [
                    const SizedBox(height: 8),
                    const Text('Terminal'),
                    const SizedBox(height: 8),
                    Expanded(
                      child: TerminalView(
                        terminal,
                        controller: controller,
                        autofocus: false,
                        textStyle: TerminalStyle(
                          fontFamily: monoFont ?? 'monospace',
                          fontSize: 14,
                        ),
                        backgroundOpacity: 1,
                        padding: const EdgeInsets.all(8),
                      ),
                    ),
                    const TerminalQuickKeyBar(
                      ctrlActive: false,
                      altActive: false,
                      onToggleModifier: _ignoreModifier,
                      onSequence: _ignoreSequence,
                    ),
                  ],
                ),
              ),
            ),
          ),
        ),
      ));
      await tester.pump();
      expect(find.text('Esc'), findsOneWidget);
      expect(find.text('Ctrl'), findsOneWidget);
      expect(find.text('PgDn'), findsOneWidget);
      expect(find.text('Ctrl+C'), findsNothing);

      final boundary = tester.renderObject<RenderRepaintBoundary>(
        find.byKey(const Key('terminal-keys-shot')),
      );
      await tester.runAsync(() async {
        final image = await boundary.toImage(pixelRatio: 1);
        final bytes = await image.toByteData(format: ui.ImageByteFormat.png);
        expect(bytes, isNotNull);
        File('/tmp/$name.png').writeAsBytesSync(bytes!.buffer.asUint8List());
      });
    }
  });
}

void _ignoreModifier(Object _) {}
void _ignoreSequence(String _) {}
