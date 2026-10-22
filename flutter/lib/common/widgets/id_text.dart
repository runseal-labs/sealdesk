import 'package:flutter/material.dart';

/// Cap used when a parent does not assign a tighter width.
/// A formatted numeric rendezvous ID at the local-ID size still fits.
const double kDeviceIdMaxWidth = 360;

/// Cap for plain-text surfaces such as OS window titles.
const int kDeviceIdTitleMaxChars = 32;

/// One-line device id. Long values ellipsize and do not widen an unbounded parent.
class IdText extends StatelessWidget {
  const IdText(
    this.text, {
    super.key,
    this.style,
    this.textAlign,
    this.maxWidth = kDeviceIdMaxWidth,
  });

  final String text;
  final TextStyle? style;
  final TextAlign? textAlign;
  final double maxWidth;

  @override
  Widget build(BuildContext context) {
    return Align(
      alignment: AlignmentDirectional.centerStart,
      widthFactor: 1,
      child: ConstrainedBox(
        constraints: BoxConstraints(maxWidth: maxWidth),
        child: Text(
          text,
          style: style,
          textAlign: textAlign,
          maxLines: 1,
          softWrap: false,
          overflow: TextOverflow.ellipsis,
        ),
      ),
    );
  }
}

/// End-ellipsis for a device id shown outside a text widget.
String ellipsizeDeviceId(String value,
    {int maxChars = kDeviceIdTitleMaxChars}) {
  final text = value.replaceAll('\n', ' ').replaceAll('\r', ' ');
  if (maxChars < 1) return '';
  if (text.length <= maxChars) return text;
  if (maxChars == 1) return '…';
  return '${text.substring(0, maxChars - 1)}…';
}
