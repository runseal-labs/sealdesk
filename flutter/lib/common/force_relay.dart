/// Menu label for the per-peer DERP switch. The key is already the English text.
const String forceRelayMenuLabel = 'Always connect via relay';

/// Stored peer option values. Empty and "N" stay off; only an explicit on value
/// forces the peer onto DERP.
bool storedPeerForceRelay(String value) => value == 'Y' || value == 'true';

/// The switch is a native tunnel control. Web has no tailcat magicsock.
bool showForceAlwaysRelayMenu({required bool web}) => !web;
