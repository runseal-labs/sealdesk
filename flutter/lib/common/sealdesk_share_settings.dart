import 'package:flutter/material.dart';

import '../common.dart';
import '../consts.dart';
import '../models/platform_model.dart';
import '../models/server_model.dart';

typedef SealDeskSetPassword = void Function({VoidCallback? notEmptyCallback});

bool sealDeskShareSettingsAllowed() =>
    !bind.isDisableSettings() &&
    bind.mainGetBuildinOption(key: kOptionHideSecuritySetting) != 'Y';

class SealDeskShareSettings extends StatefulWidget {
  final SealDeskSetPassword openSetPassword;
  final VoidCallback openOtpLength;

  /// Android share screen owns the only password refresh control.
  final bool includePasswordRefresh;

  const SealDeskShareSettings({
    super.key,
    required this.openSetPassword,
    required this.openOtpLength,
    this.includePasswordRefresh = false,
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
      passwordValue: model.serverPasswd.text,
      onRefreshPassword: widget.includePasswordRefresh
          ? () {
              bind.mainUpdateTemporaryPassword();
              model.updatePasswordModel();
            }
          : null,
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
  final String passwordValue;
  final VoidCallback? onRefreshPassword;

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
    this.passwordValue = '',
    this.onRefreshPassword,
  });

  bool get _showPassword => approveMode != 'click';

  String get _approveKey => approveMode == 'password' || approveMode == 'click'
      ? approveMode
      : 'both';

  @override
  Widget build(BuildContext context) {
    final showPermanent = _showPassword &&
        verificationMethod != kUseTemporaryPassword &&
        !permanentPasswordDisabled;
    final showOneTime =
        _showPassword && verificationMethod != kUsePermanentPassword;
    final method = verificationMethod == kUseTemporaryPassword ||
            verificationMethod == kUsePermanentPassword ||
            verificationMethod == kUseBothPasswords
        ? verificationMethod
        : kUseBothPasswords;
    final passwordText =
        passwordValue.trim().isEmpty ? '-' : passwordValue.trim();
    return Column(
      key: const ValueKey('sealdesk-share-settings'),
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Text(
          translate('Security'),
          key: const ValueKey('sealdesk-security-title'),
          style: Theme.of(context).textTheme.titleMedium,
        ),
        const SizedBox(height: 8),
        Text(
          translate('Access method'),
          key: const ValueKey('sealdesk-approve-label'),
        ),
        _menu(
          const ValueKey('sealdesk-approve'),
          _approveKey,
          [
            _item('password', 'Accept sessions via password',
                const ValueKey('sealdesk-approve-password')),
            _item('click', 'Accept sessions via click',
                const ValueKey('sealdesk-approve-click')),
            _item('both', 'Accept sessions via both',
                const ValueKey('sealdesk-approve-both')),
          ],
          approveFixed
              ? null
              : (value) {
                  if (value == null) return;
                  if (value == 'both') {
                    onApprove(defaultOptionApproveMode);
                  } else {
                    onApprove(value);
                  }
                },
        ),
        if (_showPassword) ...[
          const SizedBox(height: 8),
          Text(
            translate('Password type'),
            key: const ValueKey('sealdesk-verify-label'),
          ),
          _menu(
            const ValueKey('sealdesk-verify'),
            method,
            [
              _item(kUseTemporaryPassword, 'Use one-time password',
                  const ValueKey('sealdesk-verify-temporary')),
              _item(kUsePermanentPassword, 'Use permanent password',
                  const ValueKey('sealdesk-verify-permanent')),
              _item(kUseBothPasswords, 'Use both passwords',
                  const ValueKey('sealdesk-verify-both')),
            ],
            (value) {
              if (value != null) onVerification(value);
            },
          ),
          if (showOneTime || showPermanent) ...[
            const SizedBox(height: 8),
            Text(
              translate('Password value'),
              key: const ValueKey('sealdesk-password-value-label'),
            ),
          ],
          if (showOneTime)
            Row(
              key: const ValueKey('sealdesk-password-value'),
              children: [
                Expanded(child: Text(passwordText)),
                if (onRefreshPassword != null)
                  IconButton(
                    key: const ValueKey('sealdesk-password-refresh'),
                    tooltip: translate('Refresh password'),
                    onPressed: onRefreshPassword,
                    icon: const Icon(Icons.refresh),
                  ),
              ],
            ),
        ],
        if (showPermanent || showOneTime) ...[
          if (showPermanent)
            ListTile(
              key: const ValueKey('sealdesk-set-permanent'),
              dense: true,
              visualDensity: VisualDensity.compact,
              contentPadding: EdgeInsets.zero,
              title: Text(translate('Set permanent password')),
              trailing: const Icon(Icons.chevron_right),
              onTap: onSetPermanent,
            ),
          if (showOneTime)
            ListTile(
              key: const ValueKey('sealdesk-otp-length'),
              dense: true,
              visualDensity: VisualDensity.compact,
              contentPadding: EdgeInsets.zero,
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
              dense: true,
              visualDensity: VisualDensity.compact,
              contentPadding: EdgeInsets.zero,
              title: Text(translate('Numeric one-time password')),
              value: numeric,
              onChanged: numericFixed ? null : (_) => onToggleNumeric(),
            ),
        ],
      ],
    );
  }

  DropdownMenuItem<String> _item(String value, String label, Key key) =>
      DropdownMenuItem<String>(
        key: key,
        value: value,
        child: Text(
          translate(label),
          overflow: TextOverflow.ellipsis,
        ),
      );

  Widget _menu(
    Key key,
    String value,
    List<DropdownMenuItem<String>> items,
    ValueChanged<String?>? onChanged,
  ) {
    return DropdownButtonHideUnderline(
      child: DropdownButton<String>(
        key: key,
        isExpanded: true,
        value: value,
        items: items,
        onChanged: onChanged,
      ),
    );
  }
}
