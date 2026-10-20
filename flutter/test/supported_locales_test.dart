import 'package:flutter/widgets.dart';
import 'package:flutter_localizations/flutter_localizations.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sealdesk/common.dart' show supportedLocales;

void main() {
  const delegates = [
    GlobalMaterialLocalizations.delegate,
    GlobalWidgetsLocalizations.delegate,
    GlobalCupertinoLocalizations.delegate,
  ];

  test('supported locales are only English and Simplified Chinese', () {
    expect(supportedLocales, const [
      Locale('en', 'US'),
      Locale('en'),
      Locale('zh', 'CN'),
      Locale('zh'),
    ]);
  });

  test('system locales resolve to zh or en and stay supported', () {
    final cases = <List<Locale>, String>{
      [const Locale('zh', 'TW')]: 'zh',
      [const Locale('zh', 'CN')]: 'zh',
      [const Locale('zh')]: 'zh',
      [
        const Locale.fromSubtags(
          languageCode: 'zh',
          scriptCode: 'Hant',
          countryCode: 'TW',
        ),
        const Locale('en', 'US'),
      ]: 'zh',
      [
        const Locale.fromSubtags(
          languageCode: 'zh',
          scriptCode: 'Hans',
          countryCode: 'CN',
        ),
      ]: 'zh',
      [const Locale('de')]: 'en',
      [const Locale('de', 'DE'), const Locale('en', 'US')]: 'en',
      [const Locale('ja', 'JP')]: 'en',
      [const Locale('fr')]: 'en',
      [const Locale('en', 'GB')]: 'en',
      [const Locale('en', 'US')]: 'en',
      [const Locale('en')]: 'en',
    };

    for (final entry in cases.entries) {
      final resolved = basicLocaleListResolution(entry.key, supportedLocales);
      expect(resolved.languageCode, entry.value, reason: '${entry.key}');
      for (final delegate in delegates) {
        expect(delegate.isSupported(resolved), isTrue,
            reason: '${delegate.runtimeType} $resolved');
      }
    }

    expect(
      basicLocaleListResolution(null, supportedLocales).languageCode,
      'en',
    );
  });
}
