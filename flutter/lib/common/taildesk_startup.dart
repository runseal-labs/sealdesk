class TailDeskStartup {
  static String? _target;

  static void configure(List<String> arguments) {
    _target = arguments.length == 2 && arguments.first == '--control'
        ? arguments[1]
        : null;
  }

  static String? takeTarget() {
    final target = _target;
    _target = null;
    return target;
  }
}
