import 'package:flutter_test/flutter_test.dart';
import 'package:sealdesk/common.dart' show defaultOptionLang, effectiveLangOption;

void main() {
  test('unknown saved languages follow the system', () {
    for (final saved in ['', '   ', 'de', 'ja', 'zh-tw', 'zh_TW', 'ZH_tw', 'default']) {
      expect(effectiveLangOption(saved), defaultOptionLang, reason: saved);
    }
    expect(effectiveLangOption('zh-cn'), 'zh-cn');
    expect(effectiveLangOption('ZH-CN'), 'zh-cn');
    expect(effectiveLangOption('en'), 'en');
    expect(effectiveLangOption(' EN '), 'en');
  });
}
