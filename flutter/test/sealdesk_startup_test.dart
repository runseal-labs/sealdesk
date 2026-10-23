import 'package:sealdesk/common/sealdesk_startup.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('an install launch does not own the main-window uri server', () {
    expect(sealDeskOwnsMainWindowUriServer(['--install']), isFalse);
    expect(sealDeskOwnsMainWindowUriServer(['--cm', '--install']), isFalse);
    expect(sealDeskOwnsMainWindowUriServer([]), isTrue);
    expect(sealDeskOwnsMainWindowUriServer(['--cm']), isTrue);
    expect(sealDeskOwnsMainWindowUriServer(['--server']), isTrue);
  });

  test('only an advertised tray host keeps the process on close', () {
    expect(sealDeskTrayKeepsProcess('1'), isTrue);
    for (final value in ['', '0', 'true', 'Y']) {
      expect(sealDeskTrayKeepsProcess(value), isFalse);
    }
  });

  test('startup target is preserved verbatim and consumed once', () {
    const target = 'SEALDESK:V:1;P:秘密 &?=;;';
    SealDeskStartup.configure(['--control', target]);
    expect(SealDeskStartup.takeTarget(), target);
    expect(SealDeskStartup.takeTarget(), isNull);
    SealDeskStartup.configure(['--control', '/tmp/配对 图.png']);
    expect(SealDeskStartup.takeTarget(), '/tmp/配对 图.png');
  });

  test('other launches clear pending targets', () {
    for (final arguments in <List<String>>[
      [],
      ['--host'],
      ['--control'],
      ['--control', 'one', 'two'],
      ['multi_window', '42', 'secret'],
    ]) {
      SealDeskStartup.configure(['--control', 'pending']);
      SealDeskStartup.configure(arguments);
      expect(SealDeskStartup.takeTarget(), isNull);
    }
  });
}
