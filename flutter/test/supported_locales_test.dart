import 'package:flutter/cupertino.dart';
import 'package:flutter/material.dart';
import 'package:flutter_localizations/flutter_localizations.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sealdesk/common.dart' show supportedLocales;

void main() {
  test('supported locales are only en then zh, without country codes', () {
    expect(supportedLocales, const [
      Locale('en'),
      Locale('zh'),
    ]);
  });

  test('default resolution maps Chinese to zh and everything else to en',
      () async {
    final cases = <List<Locale>, Locale>{
      [const Locale('zh', 'CN')]: const Locale('zh'),
      [const Locale('zh', 'TW')]: const Locale('zh'),
      [const Locale('zh', 'HK')]: const Locale('zh'),
      [
        const Locale.fromSubtags(languageCode: 'zh', scriptCode: 'Hant'),
      ]: const Locale('zh'),
      [const Locale('de')]: const Locale('en'),
      [const Locale('de', 'DE')]: const Locale('en'),
      [const Locale('ja', 'JP')]: const Locale('en'),
      [const Locale('de', 'CN')]: const Locale('en'),
      [const Locale('ja', 'CN')]: const Locale('en'),
    };

    for (final entry in cases.entries) {
      final resolved = basicLocaleListResolution(entry.key, supportedLocales);
      expect(resolved, entry.value, reason: '${entry.key}');
      expect(
        GlobalMaterialLocalizations.delegate.isSupported(resolved),
        isTrue,
        reason: '${entry.key}',
      );
      expect(
        GlobalCupertinoLocalizations.delegate.isSupported(resolved),
        isTrue,
        reason: '${entry.key}',
      );

      final material =
          await GlobalMaterialLocalizations.delegate.load(resolved);
      final cupertino =
          await GlobalCupertinoLocalizations.delegate.load(resolved);
      if (entry.value == const Locale('zh')) {
        expect(material.okButtonLabel, '确定', reason: '${entry.key}');
        expect(cupertino.copyButtonLabel, '复制', reason: '${entry.key}');
      } else {
        expect(material.okButtonLabel, 'OK', reason: '${entry.key}');
        expect(cupertino.copyButtonLabel, 'Copy', reason: '${entry.key}');
      }
    }
  });
}
