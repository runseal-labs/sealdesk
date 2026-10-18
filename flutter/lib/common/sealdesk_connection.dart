String? sealDeskConnectionLabel(String streamType) {
  switch (streamType) {
    case 'Tailcat':
      return 'Checking the link';
    case 'Tailcat/P2P':
      return 'P2P direct';
    case 'Tailcat/DERP':
      return 'DERP relay';
    default:
      return null;
  }
}

String? sealDeskConnectionDescription(String streamType) {
  final label = sealDeskConnectionLabel(streamType);
  if (label == null) return null;
  final description = '$label · WireGuard encrypted connection';
  return streamType == 'Tailcat/DERP'
      ? '$description; hole punching failed, speed may be limited'
      : description;
}
