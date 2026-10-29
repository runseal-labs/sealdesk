import 'dart:io';

String? readSealDeskControlCode(String key) {
  final value = Platform.environment[key];
  if (value == null || value.trim().isEmpty) return null;
  return value.trim();
}
