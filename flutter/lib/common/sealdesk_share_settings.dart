import 'package:flutter/material.dart';

import '../common.dart';
import '../consts.dart';
import '../models/platform_model.dart';
import '../models/server_model.dart';

typedef SealDeskSetPassword = void Function({VoidCallback? notEmptyCallback});

class SealDeskShareSettings extends StatefulWidget {
  final SealDeskSetPassword openSetPassword;
  final VoidCallback openOtpLength;

  const SealDeskShareSettings({
    super.key,
    required this.openSetPassword,
    required this.openOtpLength,
  });

  @override
  State<SealDeskShareSettings> createState() => _SealDeskShareSettingsState();
}

class _SealDeskShareSettingsState extends State<SealDeskShareSettings> {
  @override
  void initState() {
    super.initState();
    gFFI.serverModel.addListener(_rebuild);
    gFFI.serverModel.updatePasswordModel();
  }

  @override
  void dispose() {
    gFFI.serverModel.removeListener(_rebuild);
    super.dispose();
  }

  void _rebuild() {
    if (mounted) setState(() {});
  }

  Future<void> _applyMethod(String value) async {
    await bind.mainSetOption(key: kOptionVerificationMethod, value: value);
    await gFFI.serverModel.updatePasswordModel();
  }

  Future<void> _onVerification(String value) async {
    if (value == kUsePermanentPassword &&
        (await bind.mainGetCommon(key: 'permanent-password-set')) != 'true') {
      if (isChangePermanentPasswordDisabled()) {
        await _applyMethod(value);
        return;
      }
      widget.openSetPassword(notEmptyCallback: () {
        _applyMethod(value);
      });
    } else {
      await _applyMethod(value);
    }
  }

  @override
  Widget build(BuildContext context) {
    final model = gFFI.serverModel;
    return SealDeskShareSettingsView(
      approveMode: model.approveMode,
      verificationMethod: model.verificationMethod,
      otpLength: model.temporaryPasswordLength,
      numeric: model.allowNumericOneTimePassword,
      approveFixed: isOptionFixed(kOptionApproveMode),
      numericFixed: isOptionFixed(kOptionAllowNumericOneTimePassword),
      permanentPasswordDisabled: isChangePermanentPasswordDisabled(),
      onApprove: (mode) async {
        await model.setApproveMode(mode);
        await model.updatePasswordModel();
      },
      onVerification: _onVerification,
      onSetPermanent: () => widget.openSetPassword(),
      onSetOtpLength: widget.openOtpLength,
      onToggleNumeric: () {
        model.switchAllowNumericOneTimePassword();
        model.updatePasswordModel();
      },
    );
  }
}

class SealDeskShareSettingsView extends StatelessWidget {
  final String approveMode;
  final String verificationMethod;
  final String otpLength;
  final bool numeric;
  final bool approveFixed;
  final bool numericFixed;
  final bool permanentPasswordDisabled;
  final Future<void> Function(String mode) onApprove;
  final Future<void> Function(String method) onVerification;
  final VoidCallback onSetPermanent;
  final VoidCallback onSetOtpLength;
  final VoidCallback onToggleNumeric;

  const SealDeskShareSettingsView({
    super.key,
    required this.approveMode,
    required this.verificationMethod,
    required this.otpLength,
    required this.numeric,
    required this.approveFixed,
    required this.numericFixed,
    required this.permanentPasswordDisabled,
    required this.onApprove,
    required this.onVerification,
    required this.onSetPermanent,
    required this.onSetOtpLength,
    required this.onToggleNumeric,
  });

  bool get _showPassword => approveMode != 'click';

  String get _approveKey =>
      approveMode == 'password' || approveMode == 'click' ? approveMode : 'both';

  @override
  Widget build(BuildContext context) {
    final showPermanent = _showPassword &&
        verificationMethod != kUseTemporaryPassword &&
        !permanentPasswordDisabled;
    final showOneTime =
        _showPassword && verificationMethod != kUsePermanentPassword;
    return Column(
      key: const ValueKey('sealdesk-share-settings'),
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Text(translate('Share settings'),
            style: Theme.of(context).textTheme.titleSmall),
        const SizedBox(height: 8),
        Card(
          margin: EdgeInsets.zero,
          child: Padding(
            padding: const EdgeInsets.symmetric(vertical: 8),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                _label(context, 'Access method'),
                _radio('password', 'Accept sessions via password', _approveKey,
                    approveFixed ? null : (value) => onApprove(value),
                    const ValueKey('sealdesk-approve-password')),
                _radio('click', 'Accept sessions via click', _approveKey,
                    approveFixed ? null : (value) => onApprove(value),
                    const ValueKey('sealdesk-approve-click')),
                _radio('both', 'Accept sessions via both', _approveKey,
                    approveFixed
                        ? null
                        : (value) => onApprove(defaultOptionApproveMode),
                    const ValueKey('sealdesk-approve-both')),
                if (_showPassword) ...[
                  _label(context, 'Password type'),
                  _radio(
                      kUseTemporaryPassword,
                      'Use one-time password',
                      verificationMethod,
                      (value) => onVerification(value),
                      const ValueKey('sealdesk-verify-temporary')),
                  _radio(
                      kUsePermanentPassword,
                      'Use permanent password',
                      verificationMethod,
                      (value) => onVerification(value),
                      const ValueKey('sealdesk-verify-permanent')),
                  _radio(
                      kUseBothPasswords,
                      'Use both passwords',
                      verificationMethod,
                      (value) => onVerification(value),
                      const ValueKey('sealdesk-verify-both')),
                ],
              ],
            ),
          ),
        ),
        if (showPermanent || showOneTime) ...[
          const SizedBox(height: 8),
          Card(
            margin: EdgeInsets.zero,
            child: Column(
              children: [
                if (showPermanent)
                  ListTile(
                    key: const ValueKey('sealdesk-set-permanent'),
                    title: Text(translate('Set permanent password')),
                    trailing: const Icon(Icons.chevron_right),
                    onTap: onSetPermanent,
                  ),
                if (showOneTime)
                  ListTile(
                    key: const ValueKey('sealdesk-otp-length'),
                    title: Text(translate('One-time password length')),
                    trailing: Row(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        Text(otpLength),
                        const Icon(Icons.chevron_right),
                      ],
                    ),
                    onTap: onSetOtpLength,
                  ),
                if (showOneTime)
                  SwitchListTile(
                    key: const ValueKey('sealdesk-otp-numeric'),
                    title: Text(translate('Numeric one-time password')),
                    value: numeric,
                    onChanged: numericFixed ? null : (_) => onToggleNumeric(),
                  ),
              ],
            ),
          ),
        ],
      ],
    );
  }

  Widget _label(BuildContext context, String key) => Padding(
        padding: const EdgeInsets.fromLTRB(16, 8, 16, 0),
        child: Text(translate(key),
            style: Theme.of(context).textTheme.labelLarge),
      );

  Widget _radio(String value, String label, String group,
          void Function(String value)? onChanged, Key key) =>
      RadioListTile<String>(
        key: key,
        dense: true,
        contentPadding: const EdgeInsets.symmetric(horizontal: 8),
        title: Text(translate(label)),
        value: value,
        groupValue: group,
        onChanged: onChanged == null ? null : (selected) {
          if (selected != null) onChanged(selected);
        },
      );
}
