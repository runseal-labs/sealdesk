import 'package:flutter_test/flutter_test.dart';
import 'package:sealdesk/desktop/macos_permission_guide.dart';

void main() {
  test('screen recording restart is prompted once after access is observed',
      () {
    expect(
      shouldPromptScreenRecordingRestart(
        watching: false,
        observed: true,
        alreadyPrompted: false,
      ),
      isFalse,
    );
    expect(
      shouldPromptScreenRecordingRestart(
        watching: true,
        observed: false,
        alreadyPrompted: false,
      ),
      isFalse,
    );
    expect(
      shouldPromptScreenRecordingRestart(
        watching: true,
        observed: true,
        alreadyPrompted: false,
      ),
      isTrue,
    );
    expect(
      shouldPromptScreenRecordingRestart(
        watching: true,
        observed: true,
        alreadyPrompted: true,
      ),
      isFalse,
    );
  });
}
