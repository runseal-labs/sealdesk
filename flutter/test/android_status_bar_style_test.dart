import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sealdesk/common/system_ui_style.dart';

const Color _darkSurface = Color(0xFF18191E);

ThemeData _lightTheme() {
  return ThemeData(
    useMaterial3: false,
    brightness: Brightness.light,
    scaffoldBackgroundColor: Colors.white,
  );
}

ThemeData _darkTheme() {
  return ThemeData(
    useMaterial3: false,
    brightness: Brightness.dark,
    scaffoldBackgroundColor: _darkSurface,
  );
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  test('icon brightness follows the surface, bars stay transparent', () {
    final light = systemOverlayStyleForTheme(_lightTheme());
    final dark = systemOverlayStyleForTheme(_darkTheme());

    expect(light.statusBarColor, Colors.transparent);
    expect(light.systemNavigationBarColor, Colors.transparent);
    expect(light.statusBarIconBrightness, Brightness.dark);
    expect(light.systemNavigationBarIconBrightness, Brightness.dark);
    expect(light.statusBarBrightness, Brightness.light);
    expect(light.systemStatusBarContrastEnforced, isFalse);
    expect(light.systemNavigationBarContrastEnforced, isFalse);

    expect(dark.statusBarIconBrightness, Brightness.light);
    expect(dark.systemNavigationBarIconBrightness, Brightness.light);
    expect(dark.statusBarBrightness, Brightness.dark);
    expect(dark.systemStatusBarContrastEnforced, isFalse);
    expect(dark.systemNavigationBarContrastEnforced, isFalse);
  });

  test('MainActivity uses NormalTheme without changing LaunchTheme', () {
    final manifest =
        File('android/app/src/main/AndroidManifest.xml').readAsStringSync();
    final activityStart = manifest.indexOf('android:name=".MainActivity"');
    final activityEnd = manifest.indexOf('</activity>', activityStart);
    final activity = manifest.substring(activityStart, activityEnd);
    expect(activity, contains('android:theme="@style/LaunchTheme"'));
    expect(
      activity,
      contains('android:name="io.flutter.embedding.android.NormalTheme"'),
    );
    expect(activity, contains('android:resource="@style/NormalTheme"'));

    const themes = [
      'android/app/src/main/res/values/styles.xml',
      'android/app/src/main/res/values-night/styles.xml',
      'android/app/src/main/res/values-v29/styles.xml',
      'android/app/src/main/res/values-night-v29/styles.xml',
    ];
    for (final path in themes) {
      final xml = File(path).readAsStringSync();
      final launch = _styleBlock(xml, 'LaunchTheme');
      final normal = _styleBlock(xml, 'NormalTheme');
      expect(launch, contains('@drawable/launch_background'), reason: path);
      expect(normal, contains('?android:colorBackground'), reason: path);
      expect(launch.contains('windowLayoutInDisplayCutoutMode'), isFalse,
          reason: path);
    }

    for (final path in themes.where((path) => path.contains('v29'))) {
      final normal = _styleBlock(File(path).readAsStringSync(), 'NormalTheme');
      expect(
        normal,
        contains('android:windowLayoutInDisplayCutoutMode">shortEdges'),
        reason: path,
      );
      expect(normal, contains('@android:color/transparent'), reason: path);
    }

    final legacy = _styleBlock(
      File('android/app/src/main/res/values/styles.xml').readAsStringSync(),
      'NormalTheme',
    );
    expect(legacy.contains('statusBarColor'), isFalse);
    expect(legacy.contains('windowLayoutInDisplayCutoutMode'), isFalse);
  });

  testWidgets('light surface keeps dark status-bar icons and clears the inset',
      (tester) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.android;
    try {
      await _pumpPhone(tester, _lightTheme());
      _expectStyle(
        statusBarIcons: Brightness.dark,
        navigationBarIcons: Brightness.dark,
      );
      _expectContentBelowStatusBar(tester);
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });

  testWidgets('dark surface keeps light status-bar icons and clears the inset',
      (tester) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.android;
    try {
      await _pumpPhone(tester, _darkTheme());
      _expectStyle(
        statusBarIcons: Brightness.light,
        navigationBarIcons: Brightness.light,
      );
      _expectContentBelowStatusBar(tester);
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });

  testWidgets('an app bar still owns status-bar icons', (tester) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.android;
    try {
      await _pumpFrame(tester);
      await tester.pumpWidget(
        MaterialApp(
          theme: _lightTheme(),
          home: EdgeToEdgeSystemUi(
            child: Scaffold(
              appBar: AppBar(
                backgroundColor: const Color(0xFF101010),
                title: const Text('Settings'),
              ),
              body: const SafeArea(child: Text('body')),
            ),
          ),
        ),
      );
      await tester.pump();

      final style = SystemChrome.latestStyle;
      expect(style, isNotNull);
      expect(style!.statusBarIconBrightness, Brightness.light);
      expect(style.systemNavigationBarIconBrightness, Brightness.dark);
      expect(style.systemNavigationBarContrastEnforced, isFalse);
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });
}

Future<void> _pumpPhone(WidgetTester tester, ThemeData theme) async {
  await _pumpFrame(tester);
  await tester.pumpWidget(
    MaterialApp(
      theme: theme,
      home: EdgeToEdgeSystemUi(
        child: const Scaffold(
          body: SafeArea(
            top: true,
            bottom: false,
            child: Align(
              alignment: Alignment.topCenter,
              child: Padding(
                padding: EdgeInsets.only(top: 8),
                child: Text('SealDesk', key: Key('content')),
              ),
            ),
          ),
        ),
      ),
    ),
  );
  await tester.pump();
}

Future<void> _pumpFrame(WidgetTester tester) async {
  tester.view.devicePixelRatio = 1;
  tester.view.physicalSize = const Size(400, 800);
  tester.view.padding = const FakeViewPadding(top: 40, bottom: 48);
  tester.view.viewPadding = const FakeViewPadding(top: 40, bottom: 48);
  addTearDown(tester.view.reset);
}

void _expectStyle({
  required Brightness statusBarIcons,
  required Brightness navigationBarIcons,
}) {
  final style = SystemChrome.latestStyle;
  expect(style, isNotNull);
  expect(style!.statusBarColor, Colors.transparent);
  expect(style.systemNavigationBarColor, Colors.transparent);
  expect(style.statusBarIconBrightness, statusBarIcons);
  expect(style.systemNavigationBarIconBrightness, navigationBarIcons);
  expect(style.systemStatusBarContrastEnforced, isFalse);
  expect(style.systemNavigationBarContrastEnforced, isFalse);
}

void _expectContentBelowStatusBar(WidgetTester tester) {
  final top = tester.getTopLeft(find.byKey(const Key('content'))).dy;
  expect(top, greaterThanOrEqualTo(40));
}

String _styleBlock(String xml, String name) {
  final start = xml.indexOf('<style name="$name"');
  expect(start, greaterThanOrEqualTo(0), reason: name);
  final end = xml.indexOf('</style>', start);
  return xml.substring(start, end);
}
