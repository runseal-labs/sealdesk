import 'dart:io';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sealdesk/mobile/widgets/terminal_quick_key_bar.dart';
import 'package:xterm/xterm.dart';

const _shotFont = 'DejaVuSans';

Future<void> _loadShotFonts() async {
  Future<void> load(String family, String path) async {
    final bytes = File(path).readAsBytesSync();
    final loader = FontLoader(family)
      ..addFont(Future.value(ByteData.sublistView(bytes)));
    await loader.load();
  }

  await load(_shotFont, '/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf');
  await load(
    'DejaVuSansMono',
    '/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf',
  );
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

      await _loadShotFonts();
      final terminal = Terminal()..write('SealDesk\r\n\$ ');
      final controller = TerminalController();
      addTearDown(controller.dispose);
      await tester.pumpWidget(MaterialApp(
        debugShowCheckedModeBanner: false,
        theme: ThemeData(
          useMaterial3: true,
          brightness: Brightness.dark,
          fontFamily: _shotFont,
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
                        textStyle: const TerminalStyle(
                          fontFamily: 'DejaVuSansMono',
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
