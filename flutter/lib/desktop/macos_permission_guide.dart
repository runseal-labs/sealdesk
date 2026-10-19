/// One-shot restart prompt after screen recording is granted.
///
/// `CGPreflightScreenCaptureAccess` can stay false in the process that already
/// observed a denial, so [observed] may come from a live window-name probe.
/// The help card still follows preflight and is not dismissed here.
bool shouldPromptScreenRecordingRestart({
  required bool watching,
  required bool observed,
  required bool alreadyPrompted,
}) {
  return watching && observed && !alreadyPrompted;
}
