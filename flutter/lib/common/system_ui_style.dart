import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

/// Status and navigation bar style for a screen that has no [AppBar].
///
/// Material edge-to-edge draws the scaffold behind transparent system bars.
/// Icon brightness follows the surface: dark icons on a light surface, light
/// icons on a dark surface. Contrast scrims stay off so Android does not paint
/// an opaque white (or black) bar over that surface.
SystemUiOverlayStyle systemOverlayStyleForTheme(ThemeData theme) {
  final bool lightSurface = theme.brightness == Brightness.light;
  final Brightness icons = lightSurface ? Brightness.dark : Brightness.light;
  return SystemUiOverlayStyle(
    statusBarColor: Colors.transparent,
    statusBarIconBrightness: icons,
    // iOS treats this as the brightness of the bar, not the icons.
    statusBarBrightness: lightSurface ? Brightness.light : Brightness.dark,
    systemStatusBarContrastEnforced: false,
    systemNavigationBarColor: Colors.transparent,
    systemNavigationBarDividerColor: Colors.transparent,
    systemNavigationBarIconBrightness: icons,
    systemNavigationBarContrastEnforced: false,
  );
}

/// Applies [systemOverlayStyleForTheme] for the surrounding [Theme].
///
/// Hit-testing prefers a descendant [AppBar] region for the status bar, so
/// pages that still have an app bar keep that bar's icon brightness.
class EdgeToEdgeSystemUi extends StatelessWidget {
  const EdgeToEdgeSystemUi({super.key, required this.child});

  final Widget child;

  @override
  Widget build(BuildContext context) {
    return AnnotatedRegion<SystemUiOverlayStyle>(
      value: systemOverlayStyleForTheme(Theme.of(context)),
      child: child,
    );
  }
}

/// Let the Flutter view draw under the system bars (Android 10+).
///
/// No-op on older Android releases, which keep the default opaque bars.
Future<void> enableAndroidEdgeToEdge() {
  return SystemChrome.setEnabledSystemUIMode(SystemUiMode.edgeToEdge);
}

/// Bring the system bars back, then return Android to edge-to-edge.
///
/// [SystemUiMode.edgeToEdge] does not restore bars hidden by immersive or
/// manual mode on Android 9 and below, so manual mode is applied first.
Future<void> restoreMobileSystemBars() async {
  await SystemChrome.setEnabledSystemUIMode(
    SystemUiMode.manual,
    overlays: SystemUiOverlay.values,
  );
  if (!kIsWeb && defaultTargetPlatform == TargetPlatform.android) {
    await enableAndroidEdgeToEdge();
  }
}
