// PR2 QR mask contract (S-AC13, decision 12). The QR card must use this
// state machine on both mobile and desktop.
//
// Auto-obscure only when the connected-controller count goes from 0 to >0.
// Manual show stays visible while controllers remain, and disconnecting does
// not change the current visibility. Start at count 0 and feed every update;
// seeding the constructor with an already-positive count skips the transition.

enum SealDeskQrTapAction { reveal, enlarge }

class SealDeskQrMask {
  bool _obscured;
  int _connectedCount;

  SealDeskQrMask({bool obscured = false, int connectedCount = 0})
      : _obscured = obscured,
        _connectedCount = connectedCount < 0 ? 0 : connectedCount;

  bool get obscured => _obscured;

  int get connectedCount => _connectedCount;

  /// True when the blur layer should show translate("Tap to show").
  bool get showsTapToShow => _obscured;

  void onConnectedCount(int count) {
    final next = count < 0 ? 0 : count;
    if (_connectedCount == 0 && next > 0) {
      _obscured = true;
    }
    _connectedCount = next;
  }

  void show() => _obscured = false;

  void hide() => _obscured = true;

  /// Obscured: reveal. Visible: enlarge, without changing the mask.
  SealDeskQrTapAction onQrTap() {
    if (_obscured) {
      show();
      return SealDeskQrTapAction.reveal;
    }
    return SealDeskQrTapAction.enlarge;
  }
}
