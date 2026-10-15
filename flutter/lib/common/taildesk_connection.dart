String? tailDeskConnectionLabel(String streamType) {
  switch (streamType) {
    case 'Tailcat':
      return '正在检测链路';
    case 'Tailcat/P2P':
      return 'P2P 直连';
    case 'Tailcat/DERP':
      return 'DERP 中继';
    default:
      return null;
  }
}

String? tailDeskConnectionDescription(String streamType) {
  final label = tailDeskConnectionLabel(streamType);
  if (label == null) return null;
  final description = '$label · WireGuard 加密连接';
  return streamType == 'Tailcat/DERP'
      ? '$description；打洞未成功，速度可能受限'
      : description;
}
