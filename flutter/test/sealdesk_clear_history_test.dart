import 'package:flutter_test/flutter_test.dart';
import 'package:sealdesk/common/sealdesk_connect.dart';
import 'package:sealdesk/common/sealdesk_connect_target.dart';
import 'package:sealdesk/generated_bridge.dart';
import 'package:sealdesk/models/platform_model.dart';

class _HistoryBinding extends Fake implements SealdeskImpl {
  final Map<String, String> hidden = {};
  var nativeHides = 0;
  var nativeOk = true;

  @override
  String mainGetLocalOption({required String key, dynamic hint}) => '';

  @override
  Future<void> mainSetLocalOption(
      {required String key, required String value, dynamic hint}) async {}

  @override
  Future<void> mainHideSealdeskRecentPeers({dynamic hint}) async {
    nativeHides++;
    if (!nativeOk) throw UnimplementedError('mainHideSealdeskRecentPeers');
  }

  @override
  Future<void> mainSetPeerOption(
      {required String id,
      required String key,
      required String value,
      dynamic hint}) async {
    if (key == kSealDeskHideRecent) hidden[id] = value;
  }
}

void main() {
  final binding = _HistoryBinding();

  setUp(() {
    platformFFI.setBindingForTesting(binding);
    binding.hidden.clear();
    binding.nativeHides = 0;
    binding.nativeOk = true;
  });

  test('clear history asks native code to mark every recent peer', () async {
    await clearSealDeskHistory(const ['only-loaded']);
    expect(binding.nativeHides, 1);
    expect(binding.hidden, isEmpty);
  });

  test('clear history marks unloaded ids when the native call is missing',
      () async {
    binding.nativeOk = false;
    await clearSealDeskHistory(const ['loaded', ' not-yet-loaded ', '']);
    expect(binding.hidden['loaded'], 'Y');
    expect(binding.hidden['not-yet-loaded'], 'Y');
    expect(binding.hidden.containsKey(''), isFalse);
    expect(binding.hidden.containsKey(' not-yet-loaded '), isFalse);
  });
}
