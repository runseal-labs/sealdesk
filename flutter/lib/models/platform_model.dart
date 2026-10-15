import 'native_model.dart' if (dart.library.html) 'web_model.dart';
import 'package:sealdesk/generated_bridge.dart'
    if (dart.library.html) 'package:sealdesk/web/bridge.dart';

final platformFFI = PlatformFFI.instance;
final localeName = PlatformFFI.localeName;

SealdeskImpl get bind => platformFFI.ffiBind;

String ffiGetByName(String name, [String arg = '']) {
  return PlatformFFI.getByName(name, arg);
}

void ffiSetByName(String name, [String value = '']) {
  PlatformFFI.setByName(name, value);
}
