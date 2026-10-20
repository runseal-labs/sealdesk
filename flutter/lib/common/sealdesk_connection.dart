String? sealDeskConnectionLabel(String streamType) {
  switch (streamType) {
    case 'Tailcat':
      return 'Checking the link';
    case 'Tailcat/P2P':
      return 'P2P direct';
    case 'Tailcat/DERP':
      return 'DERP relay';
    case 'LAN':
      return 'LAN direct';
    default:
      return null;
  }
}

String? sealDeskConnectionDescription(String streamType) {
  final label = sealDeskConnectionLabel(streamType);
  if (label == null) return null;
  if (streamType == 'LAN') {
    return '$label · encrypted connection';
  }
  final description = '$label · WireGuard encrypted connection';
  return streamType == 'Tailcat/DERP'
      ? '$description; hole punching failed, speed may be limited'
      : description;
}

/// Relay badge follows the probed stream type. An unprobed Tailcat session
/// keeps the encrypted glyph so it is not shown as direct or relay yet.
String sealDeskRelaySuffix(String streamType, String directToken) {
  switch (streamType) {
    case 'Tailcat':
    case 'Tailcat/P2P':
      return '';
    case 'Tailcat/DERP':
      return '_relay';
    case 'LAN':
      return '';
    default:
      return directToken == '_relay' ? '_relay' : '';
  }
}

String sealDeskConnectionAsset(
  String secureToken,
  String streamType,
  String directToken,
) {
  return 'assets/$secureToken${sealDeskRelaySuffix(streamType, directToken)}.svg';
}

bool? parseConnectionDirect(Object? raw) {
  if (raw == true || raw == 'true') return true;
  if (raw == false || raw == 'false') return false;
  return null;
}

bool? connectionDirectFromCache(Object? raw) {
  if (raw == null || raw is bool) return raw as bool?;
  return parseConnectionDirect(raw);
}

String tailcatStreamType(bool? direct) {
  if (direct == true) return 'Tailcat/P2P';
  if (direct == false) return 'Tailcat/DERP';
  return 'Tailcat';
}

class PublishedConnection {
  final bool secure;
  final bool? direct;
  final String streamType;

  const PublishedConnection({
    required this.secure,
    required this.direct,
    required this.streamType,
  });
}

PublishedConnection? publishedConnectionFromEvent(
  String name,
  Map<String, dynamic> evt, {
  bool secureIfPathUpdate = true,
}) {
  if (name == 'connection_ready') {
    return PublishedConnection(
      secure: evt['secure'] == 'true' || evt['secure'] == true,
      direct: parseConnectionDirect(evt['direct']),
      streamType: evt['stream_type']?.toString() ?? '',
    );
  }
  if (name == 'tailcat_path') {
    final direct = parseConnectionDirect(evt['direct']);
    final named = evt['stream_type']?.toString() ?? '';
    return PublishedConnection(
      secure: secureIfPathUpdate,
      direct: direct,
      streamType: named.isEmpty ? tailcatStreamType(direct) : named,
    );
  }
  return null;
}
