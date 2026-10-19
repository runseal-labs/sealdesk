import Cocoa
import Darwin
import QuartzCore

// Drag-into-list interaction follows PermissionFlow (MIT) by 小弟调调:
// https://github.com/jaywcjlove/PermissionFlow
// This panel is a separate AppKit implementation. It is not that package,
// and the PermissionFlow sources are not vendored. MIT terms:
// Copyright (c) 2026 小弟调调. Permission is hereby granted, free of charge,
// to deal in the software without restriction, provided the copyright notice
// and this permission notice appear in substantial portions.

private let kSystemSettingsBundle = "com.apple.systempreferences"
private let kGuideWidth: CGFloat = 92
private let kGuideHeight: CGFloat = 76

enum PermissionGuideRegistration {
    static func register() {
        guard let raw = dlsym(
            UnsafeMutableRawPointer(bitPattern: -2),
            "sealdesk_set_permission_guide_handler"
        ) else {
            return
        }
        let setter = unsafeBitCast(
            raw,
            to: (@convention(c) (@convention(c) (Int32) -> Void) -> Void).self
        )
        setter(permissionGuideEntry)
    }
}

private func permissionGuideEntry(_ kind: Int32) {
    DispatchQueue.main.async {
        PermissionGuideController.shared.show(kind: kind)
    }
}

private func statusColor(_ kind: Int32) -> NSColor {
    switch kind {
    case 1:
        return NSColor(calibratedRed: 0.20, green: 0.45, blue: 0.95, alpha: 1)
    case 2:
        return NSColor(calibratedRed: 0.55, green: 0.30, blue: 0.85, alpha: 1)
    default:
        return NSColor(calibratedRed: 0.95, green: 0.55, blue: 0.10, alpha: 1)
    }
}

final class GuidePanel: NSPanel {
    override var canBecomeKey: Bool { false }
    override var canBecomeMain: Bool { false }
}

final class PermissionGuideController: NSObject {
    static let shared = PermissionGuideController()

    private var panel: GuidePanel?
    private var effect: NSVisualEffectView?
    private var dragView: GuideDragView?
    private var timer: Timer?
    private var seenSettings = false
    private var missing = 0
    private var lastFrame: NSRect?

    func show(kind: Int32) {
        let panel = ensurePanel()
        dragView?.tint(kind)
        effect?.layer?.borderColor = statusColor(kind).cgColor
        seenSettings = false
        missing = 0
        lastFrame = nil
        if panel.frame.origin == .zero {
            if let screen = NSScreen.main ?? NSScreen.screens.first {
                let visible = screen.visibleFrame
                panel.setFrameOrigin(NSPoint(
                    x: visible.midX - kGuideWidth / 2,
                    y: visible.midY - kGuideHeight / 2
                ))
            }
        }
        panel.ignoresMouseEvents = false
        panel.alphaValue = 1
        panel.orderFrontRegardless()
        startTimer()
        poll()
    }

    func hide() {
        timer?.invalidate()
        timer = nil
        panel?.orderOut(nil)
    }

    private func ensurePanel() -> GuidePanel {
        if let panel = panel {
            return panel
        }
        let panel = GuidePanel(
            contentRect: NSRect(x: 0, y: 0, width: kGuideWidth, height: kGuideHeight),
            styleMask: [.borderless, .nonactivatingPanel],
            backing: .buffered,
            defer: false
        )
        panel.level = .floating
        panel.isReleasedWhenClosed = false
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.hasShadow = true
        panel.hidesOnDeactivate = false
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        panel.isMovable = false

        let effect = NSVisualEffectView(frame: panel.contentView?.bounds ?? .zero)
        effect.autoresizingMask = [.width, .height]
        effect.material = .hudWindow
        effect.blendingMode = .behindWindow
        effect.state = .active
        effect.wantsLayer = true
        effect.layer?.cornerRadius = 16
        effect.layer?.borderWidth = 3
        effect.layer?.masksToBounds = true

        let drag = GuideDragView(frame: effect.bounds)
        drag.autoresizingMask = [.width, .height]
        drag.onClose = { [weak self] in
            self?.hide()
        }
        effect.addSubview(drag)
        panel.contentView = effect
        self.panel = panel
        self.effect = effect
        self.dragView = drag
        return panel
    }

    private func startTimer() {
        timer?.invalidate()
        let timer = Timer(timeInterval: 0.2, repeats: true) { [weak self] _ in
            self?.poll()
        }
        RunLoop.main.add(timer, forMode: .common)
        self.timer = timer
    }

    private func poll() {
        if panel?.ignoresMouseEvents == true {
            return
        }
        guard let frame = systemSettingsFrame() else {
            missing += 1
            let limit = seenSettings ? 8 : 25
            if missing >= limit {
                hide()
            }
            return
        }
        let firstSight = !seenSettings
        seenSettings = true
        missing = 0
        guard frame != lastFrame else {
            return
        }
        lastFrame = frame
        place(near: frame)
        if firstSight {
            NSRunningApplication
                .runningApplications(withBundleIdentifier: kSystemSettingsBundle)
                .first?
                .activate(options: [.activateIgnoringOtherApps])
        }
        panel?.orderFrontRegardless()
    }

    private func place(near settings: NSRect) {
        guard let panel = panel else {
            return
        }
        let content: NSRect
        if settings.width > 520 {
            content = NSRect(
                x: settings.minX + 232,
                y: settings.minY,
                width: max(0, settings.width - 232),
                height: settings.height
            )
        } else {
            content = settings
        }
        var origin = NSPoint(
            x: content.midX - kGuideWidth / 2,
            y: settings.minY - kGuideHeight - 10
        )
        if let screen = screenContaining(settings) {
            let visible = screen.visibleFrame
            if origin.y < visible.minY {
                origin.y = settings.minY + 12
            }
            origin.x = min(max(origin.x, visible.minX + 8), visible.maxX - kGuideWidth - 8)
            origin.y = min(max(origin.y, visible.minY + 8), visible.maxY - kGuideHeight - 8)
        }
        panel.setFrameOrigin(origin)
    }
}

final class GuideDragView: NSView, NSDraggingSource {
    var onClose: (() -> Void)?
    private var kind: Int32 = 0
    private var mouseDownPoint: NSPoint?
    private var dragStarted = false
    private let closeButton = NSButton()

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        closeButton.isBordered = false
        closeButton.title = ""
        closeButton.bezelStyle = .regularSquare
        closeButton.wantsLayer = true
        closeButton.layer?.cornerRadius = 6
        closeButton.layer?.backgroundColor = NSColor(
            calibratedWhite: 0.15, alpha: 0.55
        ).cgColor
        closeButton.target = self
        closeButton.action = #selector(closePressed)
        addSubview(closeButton)
    }

    required init?(coder: NSCoder) {
        return nil
    }

    func tint(_ kind: Int32) {
        self.kind = kind
        needsDisplay = true
    }

    override func layout() {
        super.layout()
        closeButton.frame = NSRect(x: bounds.width - 22, y: bounds.height - 22, width: 12, height: 12)
    }

    override func draw(_ dirtyRect: NSRect) {
        let icon = NSWorkspace.shared.icon(forFile: Bundle.main.bundlePath)
        let iconRect = NSRect(x: (bounds.width - 48) / 2, y: 14, width: 48, height: 48)
        icon.draw(in: iconRect)
        statusColor(kind).setFill()
        NSBezierPath(ovalIn: NSRect(x: iconRect.maxX - 14, y: iconRect.minY, width: 14, height: 14)).fill()
        NSColor.white.setStroke()
        let ring = NSBezierPath(ovalIn: NSRect(x: iconRect.maxX - 14, y: iconRect.minY, width: 14, height: 14))
        ring.lineWidth = 1.5
        ring.stroke()
    }

    override func mouseDown(with event: NSEvent) {
        mouseDownPoint = convert(event.locationInWindow, from: nil)
        dragStarted = false
    }

    override func mouseDragged(with event: NSEvent) {
        guard !dragStarted, let origin = mouseDownPoint else {
            return
        }
        let point = convert(event.locationInWindow, from: nil)
        guard hypot(point.x - origin.x, point.y - origin.y) > 4 else {
            return
        }
        dragStarted = true
        beginAppDrag(with: event)
    }

    override func mouseUp(with event: NSEvent) {
        mouseDownPoint = nil
        dragStarted = false
    }

    func draggingSession(
        _ session: NSDraggingSession,
        sourceOperationMaskFor context: NSDraggingContext
    ) -> NSDragOperation {
        .copy
    }

    func ignoreModifierKeys(for session: NSDraggingSession) -> Bool {
        true
    }

    func draggingSession(_ session: NSDraggingSession, willBeginAt screenPoint: NSPoint) {
        window?.ignoresMouseEvents = true
        window?.alphaValue = 0.4
        window?.orderBack(nil)
    }

    func draggingSession(
        _ session: NSDraggingSession,
        endedAt screenPoint: NSPoint,
        operation: NSDragOperation
    ) {
        window?.ignoresMouseEvents = false
        window?.alphaValue = 1
        window?.orderFrontRegardless()
        mouseDownPoint = nil
        dragStarted = false
    }

    @objc private func closePressed() {
        onClose?()
    }

    private func beginAppDrag(with event: NSEvent) {
        let url = Bundle.main.bundleURL
        let writer = AppPathWriter(url: url)
        let item = NSDraggingItem(pasteboardWriter: writer)
        let icon = NSWorkspace.shared.icon(forFile: url.path)
        icon.size = NSSize(width: 48, height: 48)
        let point = convert(event.locationInWindow, from: nil)
        item.setDraggingFrame(
            NSRect(x: point.x - 24, y: point.y - 24, width: 48, height: 48),
            contents: icon
        )
        let session = beginDraggingSession(with: [item], event: event, source: self)
        session.animatesToStartingPositionsOnCancelOrFail = true
        session.draggingFormation = .none
    }
}

private final class AppPathWriter: NSObject, NSPasteboardWriting {
    private let url: URL

    init(url: URL) {
        self.url = url
    }

    func writableTypes(for pasteboard: NSPasteboard) -> [NSPasteboard.PasteboardType] {
        [.fileURL, NSPasteboard.PasteboardType("NSFilenamesPboardType")]
    }

    func pasteboardPropertyList(forType type: NSPasteboard.PasteboardType) -> Any? {
        switch type {
        case .fileURL:
            return url.absoluteString
        case NSPasteboard.PasteboardType("NSFilenamesPboardType"):
            return [url.path]
        default:
            return nil
        }
    }
}

private func systemSettingsFrame() -> NSRect? {
    guard let app = NSRunningApplication
        .runningApplications(withBundleIdentifier: kSystemSettingsBundle)
        .first else {
        return nil
    }
    guard let info = CGWindowListCopyWindowInfo(
        [.optionOnScreenOnly, .excludeDesktopElements],
        kCGNullWindowID
    ) as? [[String: Any]] else {
        return nil
    }
    let pid = Int(app.processIdentifier)
    var best: NSRect?
    var bestArea: CGFloat = 0
    for window in info {
        guard quartzInt(window[kCGWindowOwnerPID as String]) == pid else {
            continue
        }
        if quartzInt(window[kCGWindowLayer as String]) != 0 {
            continue
        }
        guard let bounds = windowDictionary(window[kCGWindowBounds as String]) else {
            continue
        }
        let rect = appKitRect(fromQuartz: bounds)
        let area = rect.width * rect.height
        if area > bestArea {
            bestArea = area
            best = rect
        }
    }
    return best
}

private func quartzInt(_ value: Any?) -> Int {
    if let value = value as? Int {
        return value
    }
    if let value = value as? NSNumber {
        return value.intValue
    }
    return -1
}

private func windowDictionary(_ value: Any?) -> [String: Any]? {
    if let value = value as? [String: Any] {
        return value
    }
    if let value = value as? NSDictionary {
        var dict: [String: Any] = [:]
        for (key, entry) in value {
            if let key = key as? String {
                dict[key] = entry
            }
        }
        return dict
    }
    return nil
}

private func appKitRect(fromQuartz bounds: [String: Any]) -> NSRect {
    func number(_ key: String) -> CGFloat {
        if let value = bounds[key] as? CGFloat {
            return value
        }
        if let value = bounds[key] as? Double {
            return CGFloat(value)
        }
        if let value = bounds[key] as? NSNumber {
            return CGFloat(value.doubleValue)
        }
        return 0
    }
    let x = number("X")
    let y = number("Y")
    let width = number("Width")
    let height = number("Height")
    let primary = NSScreen.screens.first { $0.frame.origin == .zero } ?? NSScreen.screens.first
    let primaryHeight = primary?.frame.height ?? 0
    return NSRect(x: x, y: primaryHeight - y - height, width: width, height: height)
}

private func screenContaining(_ rect: NSRect) -> NSScreen? {
    var best: NSScreen?
    var bestArea: CGFloat = 0
    for screen in NSScreen.screens {
        let hit = screen.frame.intersection(rect)
        let area = max(0, hit.width) * max(0, hit.height)
        if area > bestArea {
            bestArea = area
            best = screen
        }
    }
    return best ?? NSScreen.main
}
