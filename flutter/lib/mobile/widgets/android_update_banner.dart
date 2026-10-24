import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:get/get.dart';
import 'package:path_provider/path_provider.dart';

import '../../common.dart';
import '../../models/platform_model.dart';
import '../../models/state_model.dart';

/// About-section update prompt on the Android settings page. Shown only
/// after the auto-update switch has allowed a check and that check found a
/// newer stable release.
class AndroidUpdateBanner extends StatefulWidget {
  const AndroidUpdateBanner({Key? key}) : super(key: key);

  @override
  State<AndroidUpdateBanner> createState() => _AndroidUpdateBannerState();
}

class _AndroidUpdateBannerState extends State<AndroidUpdateBanner> {
  String? _dismissedUrl;
  bool _busy = false;

  Future<void> _install(String releasePageUrl) async {
    if (_busy) {
      return;
    }
    setState(() {
      _busy = true;
    });
    try {
      final cache = await getTemporaryDirectory();
      final result = await bind.mainGetCommon(
        key: 'download-verified-android-apk\n$releasePageUrl\n${cache.path}',
      );
      if (!mounted) {
        return;
      }
      if (result.startsWith('error:')) {
        _showMessage(result.substring('error:'.length));
        return;
      }
      try {
        await gFFI.invokeMethod('install_verified_apk', result);
      } on PlatformException catch (error) {
        if (!mounted) {
          return;
        }
        _showMessage(error.code == 'install_permission'
            ? translate(
                'Allow installs from this app, then try the update again')
            : (error.message ?? translate('Failed')));
      }
    } catch (error) {
      if (mounted) {
        _showMessage(error.toString());
      }
    } finally {
      if (mounted) {
        setState(() {
          _busy = false;
        });
      }
    }
  }

  void _showMessage(String message) {
    showDialog<void>(
      context: context,
      builder: (dialogContext) => AlertDialog(
        content: Text(message),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(dialogContext),
            child: Text(translate('OK')),
          ),
        ],
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    if (!isAndroid) {
      return const SizedBox.shrink();
    }
    return Obx(() {
      final updateUrl = stateGlobal.updateUrl.value;
      if (updateUrl.isEmpty || updateUrl == _dismissedUrl) {
        return const SizedBox.shrink();
      }
      final version = bind.mainGetNewVersion();
      final message =
          '${translate("new-version-of-{${bind.mainGetAppNameSync()}}-tip")} ($version).';
      return Card(
        margin: const EdgeInsets.only(top: 8, bottom: 4),
        child: Padding(
          padding: const EdgeInsets.fromLTRB(12, 4, 4, 4),
          child: Row(
            children: [
              Expanded(child: Text(message)),
              TextButton(
                onPressed: _busy ? null : () => _install(updateUrl),
                child: _busy
                    ? const SizedBox(
                        width: 16,
                        height: 16,
                        child: CircularProgressIndicator(strokeWidth: 2),
                      )
                    : Text(translate('Update')),
              ),
              IconButton(
                onPressed: _busy
                    ? null
                    : () => setState(() {
                          _dismissedUrl = updateUrl;
                        }),
                icon: const Icon(Icons.close),
              ),
            ],
          ),
        ),
      );
    });
  }
}
