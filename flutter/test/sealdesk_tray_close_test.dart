import 'package:flutter_test/flutter_test.dart';
import 'package:sealdesk/common/sealdesk_startup.dart';

void main() {
  test('main window hides to the tray only for linux SealDesk with a host', () {
    expect(
      sealDeskHidesMainWindowToTray(
        linux: true,
        sealDesk: true,
        trayHostEnv: '1',
      ),
      isTrue,
    );
    expect(
      sealDeskHidesMainWindowToTray(
        linux: true,
        sealDesk: true,
        trayHostEnv: '0',
      ),
      isFalse,
    );
    expect(
      sealDeskHidesMainWindowToTray(
        linux: true,
        sealDesk: true,
        trayHostEnv: '',
      ),
      isFalse,
    );
    expect(
      sealDeskHidesMainWindowToTray(
        linux: false,
        sealDesk: true,
        trayHostEnv: '0',
      ),
      isFalse,
    );
    expect(
      sealDeskHidesMainWindowToTray(
        linux: true,
        sealDesk: false,
        trayHostEnv: '1',
      ),
      isFalse,
    );
  });
}
