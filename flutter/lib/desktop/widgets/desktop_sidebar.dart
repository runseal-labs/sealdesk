import 'package:flutter/material.dart';

const String kDesktopSidebarWidthOption = 'desktop-sidebar-width';
const double kDesktopSidebarMinWidth = 200;
const double kDesktopSidebarMaxWidth = 480;
const double kDesktopSidebarDefaultWidth = 300;
const double kDesktopSidebarRightMinWidth = 280;
const double kDesktopSidebarDividerExtent = 7;

double? parseDesktopSidebarWidth(String raw) {
  final value = double.tryParse(raw.trim());
  if (value == null || !value.isFinite || value <= 0) return null;
  return value;
}

double clampDesktopSidebarWidth(double width, double windowWidth) {
  final available =
      windowWidth - kDesktopSidebarDividerExtent - kDesktopSidebarRightMinWidth;
  final upper =
      available < kDesktopSidebarMaxWidth ? available : kDesktopSidebarMaxWidth;
  final safeUpper = upper < 0 ? 0.0 : upper;
  final lower =
      safeUpper < kDesktopSidebarMinWidth ? safeUpper : kDesktopSidebarMinWidth;
  if (!width.isFinite) return lower;
  return width.clamp(lower, safeUpper).toDouble();
}

class DesktopSidebarScaffold extends StatefulWidget {
  const DesktopSidebarScaffold({
    super.key,
    required this.savedWidth,
    required this.defaultWidth,
    required this.onSave,
    required this.scrollController,
    required this.leftChildren,
    required this.right,
    this.split = true,
    this.fixedWidth = kDesktopSidebarDefaultWidth,
    this.columnKey,
    this.background,
    this.overlay,
  });

  final String savedWidth;
  final double defaultWidth;
  final ValueChanged<String> onSave;
  final ScrollController scrollController;
  final List<Widget> leftChildren;
  final Widget right;
  final bool split;
  final double fixedWidth;
  final Key? columnKey;
  final Color? background;
  final Widget? overlay;

  @override
  State<DesktopSidebarScaffold> createState() => _DesktopSidebarScaffoldState();
}

class _DesktopSidebarScaffoldState extends State<DesktopSidebarScaffold> {
  late double _preferred;
  bool _userSet = false;
  double _windowWidth = 0;

  @override
  void initState() {
    super.initState();
    final saved = parseDesktopSidebarWidth(widget.savedWidth);
    if (saved != null) {
      _preferred = saved;
      _userSet = true;
    } else {
      _preferred = widget.defaultWidth;
    }
  }

  @override
  void didUpdateWidget(DesktopSidebarScaffold oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (_userSet) return;
    if (oldWidget.defaultWidth != widget.defaultWidth) {
      _preferred = widget.defaultWidth;
    }
  }

  void _drag(double dx) {
    final next = clampDesktopSidebarWidth(
        clampDesktopSidebarWidth(_preferred, _windowWidth) + dx, _windowWidth);
    if (next == _preferred) return;
    setState(() {
      _preferred = next;
      _userSet = true;
    });
  }

  void _save() {
    if (!widget.split || _windowWidth <= 0) return;
    final shown = clampDesktopSidebarWidth(_preferred, _windowWidth);
    _preferred = shown;
    _userSet = true;
    widget.onSave(shown.toStringAsFixed(1));
  }

  Widget _pane(double width) {
    return DesktopSidebarPane(
      width: width,
      controller: widget.scrollController,
      columnKey: widget.columnKey,
      background: widget.background,
      overlay: widget.overlay,
      children: widget.leftChildren,
    );
  }

  @override
  Widget build(BuildContext context) {
    if (!widget.split) {
      return _pane(widget.fixedWidth);
    }
    return LayoutBuilder(builder: (context, constraints) {
      _windowWidth = constraints.maxWidth;
      final width = clampDesktopSidebarWidth(_preferred, _windowWidth);
      return Row(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _pane(width),
          _SidebarDivider(onDrag: _drag, onDragEnd: _save),
          Expanded(child: widget.right),
        ],
      );
    });
  }
}

class DesktopSidebarPane extends StatelessWidget {
  const DesktopSidebarPane({
    super.key,
    required this.width,
    required this.controller,
    required this.children,
    this.columnKey,
    this.background,
    this.overlay,
  });

  final double width;
  final ScrollController controller;
  final List<Widget> children;
  final Key? columnKey;
  final Color? background;
  final Widget? overlay;

  @override
  Widget build(BuildContext context) {
    return SizedBox(
      key: const ValueKey('desktop-sidebar-pane'),
      width: width,
      child: ColoredBox(
        color: background ?? Theme.of(context).colorScheme.background,
        child: Stack(
          children: [
            SingleChildScrollView(
              key: const ValueKey('desktop-sidebar-scroll'),
              controller: controller,
              child: Column(
                key: columnKey,
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: children,
              ),
            ),
            if (overlay != null) overlay!,
          ],
        ),
      ),
    );
  }
}

class _SidebarDivider extends StatefulWidget {
  const _SidebarDivider({required this.onDrag, required this.onDragEnd});

  final ValueChanged<double> onDrag;
  final VoidCallback onDragEnd;

  @override
  State<_SidebarDivider> createState() => _SidebarDividerState();
}

class _SidebarDividerState extends State<_SidebarDivider> {
  int? _pointer;

  @override
  Widget build(BuildContext context) {
    return Listener(
      onPointerDown: (event) => _pointer = event.pointer,
      onPointerMove: (event) {
        if (_pointer != event.pointer) return;
        widget.onDrag(event.delta.dx);
      },
      onPointerUp: (event) {
        if (_pointer != event.pointer) return;
        _pointer = null;
        widget.onDragEnd();
      },
      onPointerCancel: (event) {
        if (_pointer == event.pointer) _pointer = null;
      },
      child: MouseRegion(
        cursor: SystemMouseCursors.resizeLeftRight,
        child: const SizedBox(
          key: ValueKey('desktop-sidebar-divider'),
          width: kDesktopSidebarDividerExtent,
          child: DecoratedBox(
            decoration: BoxDecoration(
              border: Border(
                left: BorderSide(
                  color: Color.fromARGB(200, 177, 175, 175),
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}
