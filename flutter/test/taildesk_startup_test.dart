import 'package:flutter_hbb/common/taildesk_startup.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('startup target is preserved verbatim and consumed once', () {
    const target = 'TAILDESK:V:1;P:秘密 &?=;;';
    TailDeskStartup.configure(['--control', target]);
    expect(TailDeskStartup.takeTarget(), target);
    expect(TailDeskStartup.takeTarget(), isNull);
    TailDeskStartup.configure(['--control', '/tmp/配对 图.png']);
    expect(TailDeskStartup.takeTarget(), '/tmp/配对 图.png');
  });

  test('other launches clear pending targets', () {
    for (final arguments in <List<String>>[
      [],
      ['--host'],
      ['--control'],
      ['--control', 'one', 'two'],
      ['multi_window', '42', 'secret'],
    ]) {
      TailDeskStartup.configure(['--control', 'pending']);
      TailDeskStartup.configure(arguments);
      expect(TailDeskStartup.takeTarget(), isNull);
    }
  });
}
