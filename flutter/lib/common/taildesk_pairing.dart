class TailDeskPairing {
  final String address;
  final String? password;
  final String? name;
  final int? expiresAt;

  const TailDeskPairing({
    required this.address,
    this.password,
    this.name,
    this.expiresAt,
  });

  static bool isAddress(String value) =>
      value.length > 40 && RegExp(r'^tc[A-Za-z0-9_-]+$').hasMatch(value);

  static bool isPairingCode(String value) =>
      value.trimLeft().toUpperCase().startsWith('TAILDESK:');

  static int _now() => DateTime.now().millisecondsSinceEpoch ~/ 1000;

  static bool _special(String value) =>
      const ['\\', ';', ',', ':', '"'].contains(value);

  factory TailDeskPairing.parse(String input, {int? now}) {
    final text = input.trim();
    if (!isPairingCode(text)) {
      throw const FormatException('不是有效的 TailDesk 配对码');
    }
    final fields = <String, String>{};
    var field = StringBuffer();
    var escaped = false;
    var finished = false;
    for (var index = 9; index < text.length; index++) {
      final character = text[index];
      if (escaped) {
        if (!_special(character)) {
          throw const FormatException('配对码包含无效转义');
        }
        field.write(character);
        escaped = false;
      } else if (character == '\\') {
        if (!field.toString().contains(':')) {
          throw const FormatException('配对码包含无效转义');
        }
        escaped = true;
      } else if (character == ';') {
        if (field.isEmpty) {
          if (index != text.length - 1) {
            throw const FormatException('配对码结束标记后包含多余内容');
          }
          finished = true;
          break;
        }
        final value = field.toString();
        final separator = value.indexOf(':');
        if (separator <= 0) {
          throw const FormatException('配对码字段格式无效');
        }
        final key = value.substring(0, separator);
        if (const ['V', 'A', 'P', 'N', 'E'].contains(key)) {
          if (fields.containsKey(key)) {
            throw FormatException('配对码字段重复：$key');
          }
          fields[key] = value.substring(separator + 1);
        }
        field = StringBuffer();
      } else {
        field.write(character);
      }
    }
    if (escaped || !finished) {
      throw const FormatException('配对码不完整，请重新复制或扫描');
    }
    if (!fields.containsKey('V') || !fields.containsKey('A')) {
      throw const FormatException('配对码缺少版本或地址');
    }
    if (fields['V'] != '1') {
      throw const FormatException('配对码版本不受支持，请升级 TailDesk');
    }
    int? expiry;
    if (fields.containsKey('E')) {
      final value = fields['E']!;
      expiry = int.tryParse(value);
      if (!RegExp(r'^[0-9]+$').hasMatch(value) ||
          expiry == null ||
          expiry < 0) {
        throw const FormatException('配对码的过期时间无效');
      }
    }
    final pairing = TailDeskPairing(
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
      throw const FormatException('配对码中的地址无效');
    }
    if (expiresAt != null && expiresAt! <= now) {
      throw const FormatException('配对码已过期，请让对方刷新二维码');
    }
  }

  String encode({int? now}) {
    _validate(now ?? _now());
    final output = StringBuffer('TAILDESK:V:1;A:$address;');
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
