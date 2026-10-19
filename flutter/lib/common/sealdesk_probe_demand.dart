import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';

import '../models/platform_model.dart';

/// `share=0|1,background=0|1,screen=0|1` for the tailcat self-probe.
String sealDeskProbeDemandValue({
  required bool shareVisible,
  required bool background,
  required bool screenOff,
}) =>
    'share=${shareVisible ? 1 : 0},background=${background ? 1 : 0},screen=${screenOff ? 1 : 0}';

/// Paused, hidden, and detached all mean the UI is not in front of the user.
/// Flutter does not report screen-off separately from background, so both
/// flags follow this. Inactive is a transient overlay and does not count.
bool sealDeskLifecycleAsleep(AppLifecycleState state) {
  switch (state) {
    case AppLifecycleState.paused:
    case AppLifecycleState.detached:
    case AppLifecycleState.hidden:
      return true;
    case AppLifecycleState.resumed:
    case AppLifecycleState.inactive:
      return false;
  }
}

/// Share-page visibility and app lifecycle for the host self-probe.
class SealDeskProbeDemand {
  static bool shareVisible = false;
  static bool background = false;
  static bool screenOff = false;
  static String? _last;
  static int _seq = 0;
  static Future<void> _pending = Future<void>.value();

  /// Replaces the desktop/mobile option write in tests.
  @visibleForTesting
  static Future<void> Function(String value)? debugWriteForTesting;

  static void setShareVisible(bool value) {
    if (shareVisible == value) {
      publish();
      return;
    }
    shareVisible = value;
    publish();
  }

  static void setLifecycle(AppLifecycleState state) {
    final asleep = sealDeskLifecycleAsleep(state);
    background = asleep;
    screenOff = asleep;
    publish();
  }

  /// [repeat] resends the same flags. The share page does this on its existing
  /// poll so a desktop server that restarted still learns the current demand.
  ///
  /// Writes are ordered. A demand that has not started is dropped once a newer
  /// one exists, and only one [bind.mainSetOption] is in flight. Desktop
  /// delivery returns after the service applies that value, so a slow older
  /// write cannot be the one left in effect.
  static void publish({bool repeat = false}) {
    final value = sealDeskProbeDemandValue(
      shareVisible: shareVisible,
      background: background,
      screenOff: screenOff,
    );
    if (!repeat && value == _last) return;
    _last = value;
    final seq = ++_seq;
    _pending = _pending.catchError((Object _) {}).then((_) async {
      if (seq != _seq) return;
      final write = debugWriteForTesting;
      if (write != null) {
        await write(value);
        return;
      }
      await bind.mainSetOption(key: 'tailcat-probe-demand', value: value);
    });
  }

  @visibleForTesting
  static Future<void> debugDrainForTesting() => _pending;

  @visibleForTesting
  static void debugResetForTesting() {
    shareVisible = false;
    background = false;
    screenOff = false;
    _last = null;
    _seq = 0;
    _pending = Future<void>.value();
    debugWriteForTesting = null;
  }
}

/// Mounted only with the share page. Disposing it stops idle probes.
class SealDeskProbeScope extends StatefulWidget {
  final Widget child;

  const SealDeskProbeScope({required this.child, super.key});

  @override
  State<SealDeskProbeScope> createState() => _SealDeskProbeScopeState();
}

class _SealDeskProbeScopeState extends State<SealDeskProbeScope> {
  @override
  void initState() {
    super.initState();
    SealDeskProbeDemand.setShareVisible(true);
  }

  @override
  void dispose() {
    SealDeskProbeDemand.setShareVisible(false);
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => widget.child;
}
