class SealDeskPairing {
  final String address;
  final String? password;
  final String? name;
  final int? expiresAt;

  const SealDeskPairing({
    required this.address,
    this.password,
    this.name,
    this.expiresAt,
  });

  static bool isAddress(String value) =>
      value.length > 40 && RegExp(r'^tc[A-Za-z0-9_-]+$').hasMatch(value);

  static bool isPairingCode(String value) =>
      value.trimLeft().toUpperCase().startsWith('SEALDESK:');

  static int _now() => DateTime.now().millisecondsSinceEpoch ~/ 1000;

  static bool _special(String value) =>
      const ['\\', ';', ',', ':', '"'].contains(value);

  factory SealDeskPairing.parse(String input, {int? now}) {
    final text = input.trim();
    if (!isPairingCode(text)) {
      throw const FormatException('Not a valid SealDesk pairing code');
    }
    final fields = <String, String>{};
    var field = StringBuffer();
    var escaped = false;
    var finished = false;
    for (var index = 9; index < text.length; index++) {
      final character = text[index];
      if (escaped) {
        if (!_special(character)) {
          throw const FormatException(
              'Pairing code contains an invalid escape');
        }
        field.write(character);
        escaped = false;
      } else if (character == '\\') {
        if (!field.toString().contains(':')) {
          throw const FormatException(
              'Pairing code contains an invalid escape');
        }
        escaped = true;
      } else if (character == ';') {
        if (field.isEmpty) {
          if (index != text.length - 1) {
            throw const FormatException(
                'Pairing code has extra content after the end marker');
          }
          finished = true;
          break;
        }
        final value = field.toString();
        final separator = value.indexOf(':');
        if (separator <= 0) {
          throw const FormatException('Pairing code field format is invalid');
        }
        final key = value.substring(0, separator);
        if (const ['V', 'A', 'P', 'N', 'E'].contains(key)) {
          if (fields.containsKey(key)) {
            throw FormatException('Duplicate pairing field: {$key}');
          }
          fields[key] = value.substring(separator + 1);
        }
        field = StringBuffer();
      } else {
        field.write(character);
      }
    }
    if (escaped || !finished) {
      throw const FormatException(
          'Pairing code is incomplete. Copy or scan it again');
    }
    if (!fields.containsKey('V') || !fields.containsKey('A')) {
      throw const FormatException(
          'Pairing code is missing a version or address');
    }
    if (fields['V'] != '1') {
      throw const FormatException(
          'Pairing code version is not supported. Please upgrade SealDesk');
    }
    int? expiry;
    if (fields.containsKey('E')) {
      final value = fields['E']!;
      expiry = int.tryParse(value);
      if (!RegExp(r'^[0-9]+$').hasMatch(value) ||
          expiry == null ||
          expiry < 0) {
        throw const FormatException('Pairing code expiry is invalid');
      }
    }
    final pairing = SealDeskPairing(
      address: fields['A']!,
      password: fields['P'],
      name: fields['N'],
      expiresAt: expiry,
    );
    pairing._validate(now ?? _now());
    return pairing;
  }

  void _validate(int now) {
    if (!isAddress(address)) {
      throw const FormatException('Pairing code address is invalid');
    }
    if (expiresAt != null && expiresAt! <= now) {
      throw const FormatException(
          'Pairing code has expired. Ask the other person to refresh the QR code');
    }
  }

  String encode({int? now}) {
    _validate(now ?? _now());
    final output = StringBuffer('SEALDESK:V:1;A:$address;');
    for (final entry in {'P': password, 'N': name}.entries) {
      final value = entry.value;
      if (value == null) continue;
      output.write('${entry.key}:');
      for (final character in value.split('')) {
        if (_special(character)) output.write('\\');
        output.write(character);
      }
      output.write(';');
    }
    if (expiresAt != null) output.write('E:$expiresAt;');
    output.write(';');
    return output.toString();
  }
}
