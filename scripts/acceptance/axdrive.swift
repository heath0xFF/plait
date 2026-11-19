// axdrive: the native acceptance check's hands and eyes.
//
// Every command that touches an application takes the process ID of the
// development copy, so the person's own Plait (same executable name) is never
// addressed. Controls are found and pressed through the accessibility API,
// which does not move the pointer. Keys are posted to that one process, not to
// whatever is frontmost. Pointer commands are separate, refuse to act unless
// the development copy's window is the one under the point, and put the
// pointer back afterwards.
//
// Built by scripts/acceptance-macos.sh with `xcrun swiftc`. It is a
// development tool and is not part of the app.

import AppKit
import ApplicationServices
import Foundation

// MARK: - Output and errors

/// Exit codes: 0 done, 1 not seen or not done, 2 usage or permission, 3 refused.
func die(_ message: String, _ code: Int32 = 1) -> Never {
    FileHandle.standardError.write(Data((message + "\n").utf8))
    exit(code)
}

func say(_ line: String) {
    print(line)
}

func fmt(_ value: CGFloat) -> String {
    String(format: "%.1f", Double(value))
}

func fmt(_ rect: CGRect) -> String {
    "\(fmt(rect.minX)) \(fmt(rect.minY)) \(fmt(rect.width)) \(fmt(rect.height))"
}

// MARK: - Selectors (pure, covered by self-test)

/// Which elements a command acts on. Every given criterion must hold.
struct Selector: Equatable {
    var roles: [String] = []
    var exact: String?
    var prefix: String?
    var contains: String?
    var regex: String?
    var nth = 1
    var timeout: Double = 3

    var describe: String {
        var parts: [String] = []
        if !roles.isEmpty { parts.append("role \(roles.joined(separator: "|"))") }
        if let exact { parts.append("named \"\(exact)\"") }
        if let prefix { parts.append("starting \"\(prefix)\"") }
        if let contains { parts.append("containing \"\(contains)\"") }
        if let regex { parts.append("matching /\(regex)/") }
        if nth != 1 { parts.append("number \(nth)") }
        return parts.isEmpty ? "any element" : parts.joined(separator: ", ")
    }

    func matches(role: String, names: [String]) -> Bool {
        if !roles.isEmpty && !roles.contains(role) { return false }
        if exact == nil && prefix == nil && contains == nil && regex == nil { return true }
        return names.contains { name in
            if let exact, name != exact { return false }
            if let prefix, !name.hasPrefix(prefix) { return false }
            if let contains, !name.contains(contains) { return false }
            if let regex, name.range(of: regex, options: .regularExpression) == nil { return false }
            return true
        }
    }
}

/// Shorthands so callers need not know each AppKit role a GPUI role maps to.
func expandRoles(_ text: String) -> [String] {
    text.split(separator: ",").flatMap { part -> [String] in
        switch part {
        case "control":
            return ["AXButton", "AXRadioButton", "AXCheckBox", "AXPopUpButton",
                    "AXMenuButton", "AXLink", "AXRow", "AXTab", "AXDisclosureTriangle"]
        case "field":
            return ["AXTextField", "AXTextArea", "AXSearchField", "AXComboBox"]
        case "text":
            return ["AXStaticText"]
        default:
            return [String(part)]
        }
    }
}

/// Splits `--option value` pairs into a selector and the remaining words.
func parseSelector(_ args: [String]) throws -> (Selector, [String: String], [String]) {
    var selector = Selector()
    var extra: [String: String] = [:]
    var rest: [String] = []
    var index = 0
    func value(_ name: String) throws -> String {
        index += 1
        guard index < args.count else { throw UsageError("\(name) needs a value") }
        return args[index]
    }
    while index < args.count {
        let word = args[index]
        switch word {
        case "--role": selector.roles = expandRoles(try value(word))
        case "--name": selector.exact = try value(word)
        case "--prefix": selector.prefix = try value(word)
        case "--contains": selector.contains = try value(word)
        case "--regex":
            let pattern = try value(word)
            guard (try? NSRegularExpression(pattern: pattern)) != nil else {
                throw UsageError("invalid --regex \(pattern)")
            }
            selector.regex = pattern
        case "--nth":
            guard let nth = Int(try value(word)), nth >= 1 else { throw UsageError("--nth needs a positive number") }
            selector.nth = nth
        case "--timeout":
            guard let seconds = Double(try value(word)), seconds >= 0 else { throw UsageError("--timeout needs seconds") }
            selector.timeout = seconds
        case "--text", "--delay":
            extra[word] = try value(word)
        default:
            if word.hasPrefix("--") { throw UsageError("unknown option \(word)") }
            rest.append(word)
        }
        index += 1
    }
    return (selector, extra, rest)
}

struct UsageError: Error, CustomStringConvertible {
    let description: String
    init(_ description: String) { self.description = description }
}

// MARK: - Key combinations (pure, covered by self-test)

/// US layout virtual key codes; the shortcuts under test are defined by key.
let keyCodes: [String: CGKeyCode] = [
    "a": 0, "s": 1, "d": 2, "f": 3, "h": 4, "g": 5, "z": 6, "x": 7, "c": 8, "v": 9,
    "b": 11, "q": 12, "w": 13, "e": 14, "r": 15, "y": 16, "t": 17, "1": 18, "2": 19,
    "3": 20, "4": 21, "6": 22, "5": 23, "9": 25, "7": 26, "8": 28, "0": 29, "o": 31,
    "u": 32, "i": 34, "p": 35, "return": 36, "enter": 36, "l": 37, "j": 38, "k": 40,
    "comma": 43, "slash": 44, "n": 45, "m": 46, "period": 47, "tab": 48, "space": 49,
    "delete": 51, "escape": 53, "left": 123, "right": 124, "down": 125, "up": 126,
]

struct Combo: Equatable {
    let key: CGKeyCode
    let flags: CGEventFlags
}

/// Parses `cmd+shift+2`, `cmd+opt+period`, `escape`.
func parseCombo(_ text: String) throws -> Combo {
    var flags: CGEventFlags = []
    var key: CGKeyCode?
    for part in text.lowercased().split(separator: "+").map(String.init) {
        switch part {
        case "cmd", "command": flags.insert(.maskCommand)
        case "shift": flags.insert(.maskShift)
        case "opt", "option", "alt": flags.insert(.maskAlternate)
        case "ctrl", "control": flags.insert(.maskControl)
        default:
            guard key == nil, let code = keyCodes[part] else { throw UsageError("unknown key in \(text)") }
            key = code
        }
    }
    guard let key else { throw UsageError("no key in \(text)") }
    return Combo(key: key, flags: flags)
}

/// Text as key events: one character (grapheme) per event, as typing sends it.
func typingUnits(_ text: String) -> [[UniChar]] {
    text.map { Array(String($0).utf16) }
}

// MARK: - Finding the tab row's empty right end (pure, covered by self-test)

struct BarElement: Equatable {
    let role: String
    let frame: CGRect
}

enum TabBarEnd: Equatable {
    case point(CGPoint, left: CGFloat, right: CGFloat)
    case noEmptyEnd(rightmost: CGFloat, windowRight: CGFloat, available: CGFloat)
    case noControls
}

/// Finds a point inside the 40-point top tab row's empty right end from the
/// accessibility tree's own frames: strictly right of every control in the top
/// band (traffic lights, sidebar toggle, tabs, overflow, New thread tab) and
/// inside the window's right edge.
func tabBarDragPoint(
    window: CGRect,
    elements: [BarElement],
    barHeight: CGFloat = 40,
    padding: CGFloat = 8,
    minimum: CGFloat = 16
) -> TabBarEnd {
    let band = CGRect(x: window.minX, y: window.minY, width: window.width, height: barHeight)
    let barRoles = Set(expandRoles("control,field,text,AXSplitter,AXTabGroup"))
    var rightmost = window.minX
    var sawControl = false
    for element in elements {
        let box = element.frame
        guard box.width > 0, box.height > 0, box.height <= barHeight + 6,
              box.intersects(band), barRoles.contains(element.role) else { continue }
        sawControl = true
        rightmost = max(rightmost, box.maxX)
    }
    guard sawControl else { return .noControls }
    let left = rightmost + padding
    let right = window.maxX - padding
    let available = max(0, window.maxX - rightmost)
    guard right - left >= minimum else {
        return .noEmptyEnd(rightmost: rightmost, windowRight: window.maxX, available: available)
    }
    return .point(CGPoint(x: (left + right) / 2, y: window.minY + barHeight / 2), left: left, right: right)
}

/// Reads an `axdrive dump` back into window and element frames, so the drag
/// point can be checked against trees recorded from real runs.
func barElements(dump: String) -> (window: CGRect, elements: [BarElement])? {
    var window: CGRect?
    var elements: [BarElement] = []
    for line in dump.split(separator: "\n") {
        let text = line.trimmingCharacters(in: .whitespaces)
        guard let range = text.range(of: " frame=", options: .backwards) else { continue }
        let values = text[range.upperBound...].split(separator: " ").compactMap { Double($0) }
        guard values.count == 4 else { continue }
        let box = CGRect(x: values[0], y: values[1], width: values[2], height: values[3])
        let role = String(text.prefix { $0 != "/" && $0 != " " })
        if role == "AXWindow" { window = window ?? box } else { elements.append(BarElement(role: role, frame: box)) }
    }
    return window.map { ($0, elements) }
}

/// Deduplicated wait trace so timed-out selection checks show what was observed
/// at every distinct state transition without flooding logs.
struct WaitTrace {
    private(set) var polls = 0
    private(set) var events: [(Double, String)] = []
    private var lastState: String?

    mutating func record(at elapsed: Double, state: String) {
        polls += 1
        if state != lastState {
            lastState = state
            if events.count < 12 {
                events.append((elapsed, state))
            }
        }
    }

    var last: String {
        lastState ?? "no observations"
    }

    var summary: String {
        let entries = events.map { String(format: "%.2fs: %@", $0.0, $0.1) }.joined(separator: " -> ")
        return "polls=\(polls); states=[\(entries)]"
    }
}

/// Tracks streaming-label observations so early disappearance is distinguished
/// from a genuine timeout.
struct GrowthTrace {
    private(set) var polls = 0
    private(set) var lengths = Set<Int>()
    private(set) var firstSeenAt: Double?
    private(set) var lastSeenAt: Double?
    private(set) var vanishedAt: Double?
    private(set) var samples: [(elapsed: Double, length: Int, role: String, frame: String)] = []
    private var lastLength: Int?

    var seen: Bool { firstSeenAt != nil }
    var grew: Bool { seen && lengths.count >= 2 }

    mutating func observePresent(at elapsed: Double, length: Int, role: String, frame: String) {
        polls += 1
        if firstSeenAt == nil { firstSeenAt = elapsed }
        lastSeenAt = elapsed
        lengths.insert(length)
        if length != lastLength {
            lastLength = length
            if samples.count < 16 {
                samples.append((elapsed, length, role, frame))
            }
        }
    }

    mutating func observeAbsent(at elapsed: Double) -> Bool {
        polls += 1
        if seen && vanishedAt == nil {
            vanishedAt = elapsed
            return true
        }
        return false
    }

    var sampleDescription: String {
        if samples.isEmpty { return "none" }
        return samples.map {
            String(format: "%.2fs:len=%d(%@ frame=[%@])", $0.elapsed, $0.length, $0.role, $0.frame)
        }.joined(separator: ", ")
    }

    func failureMessage(timeout: Double, describe: String) -> String {
        if let vanished = vanishedAt {
            let first = firstSeenAt.map { String(format: "%.2fs", $0) } ?? "?"
            let last = lastSeenAt.map { String(format: "%.2fs", $0) } ?? "?"
            return "element \(describe) disappeared at \(String(format: "%.2fs", vanished)) before growing (not a timeout; first=\(first) last=\(last)); polls=\(polls); observed=[\(sampleDescription)]"
        }
        return "timed out after \(timeout)s waiting for growth of \(describe); polls=\(polls); last observed: \(seen ? "one length, \(lengths.max() ?? 0) characters" : "control absent"); observed=[\(sampleDescription)]"
    }
}

/// AX and CGEvent use top-left screen points; AppKit hit testing uses bottom-left.
/// Never apply the display's pixel scale to either coordinate system.
func flipScreenPoint(_ point: CGPoint, primaryTop: CGFloat) -> CGPoint {
    CGPoint(x: point.x, y: primaryTop - point.y)
}

func topLeftFrame(_ rect: CGRect, primaryTop: CGFloat) -> CGRect {
    CGRect(x: rect.minX, y: primaryTop - rect.maxY, width: rect.width, height: rect.height)
}

func placement(width: CGFloat, height: CGFloat, visible: CGRect) -> CGRect {
    CGRect(x: visible.minX + 20, y: visible.minY + 20,
           width: min(width, visible.width - 40), height: min(height, visible.height - 40))
}

struct WindowInfo {
    let number: Int
    let pid: pid_t
    let owner: String
    let frame: CGRect
    let layer: Int
    let alpha: Double

    var description: String {
        "owner=\(owner) pid=\(pid) window=\(number) frame=[\(fmt(frame))] layer=\(layer)"
    }
}

func permitsPointer(target: WindowInfo?, pid: pid_t, window: Int?, frame: CGRect, point: CGPoint) -> Bool {
    guard let target, let window else { return false }
    return target.pid == pid && target.number == window && frame.contains(point) && target.frame.contains(point)
}

// MARK: - Accessibility tree

struct Node {
    let element: AXUIElement
    let role: String
    let subrole: String
    let title: String
    let label: String
    let value: String?
    let frame: CGRect?
    let depth: Int

    /// Compare AppKit names and textual values. The pinned AccessKit adapter
    /// exposes GPUI `aria_label` as the title.
    var names: [String] {
        [title, label, value ?? ""].filter { !$0.isEmpty }
    }

    var name: String {
        names.first ?? ""
    }
}

let fetched = [
    kAXRoleAttribute, kAXSubroleAttribute, kAXTitleAttribute, kAXDescriptionAttribute,
    kAXValueAttribute, kAXPositionAttribute, kAXSizeAttribute, kAXChildrenAttribute,
] as CFArray

func attribute(_ element: AXUIElement, _ name: String) -> CFTypeRef? {
    var value: CFTypeRef?
    guard AXUIElementCopyAttributeValue(element, name as CFString, &value) == .success else { return nil }
    return value
}

func flagValue(_ value: CFTypeRef?) -> Bool? {
    guard let value, CFGetTypeID(value) == CFBooleanGetTypeID() || CFGetTypeID(value) == CFNumberGetTypeID(),
          let number = value as? NSNumber, [0.0, 1.0].contains(number.doubleValue) else { return nil }
    return number.boolValue
}

func rawAttribute(_ element: AXUIElement, _ name: String) -> String {
    var value: CFTypeRef?
    let status = AXUIElementCopyAttributeValue(element, name as CFString, &value)
    guard status == .success, let value else { return "unavailable(AXError=\(status.rawValue))" }
    if let flag = flagValue(value) { return flag ? "true(1)" : "false(0)" }
    return "\(value) [type=\(CFGetTypeID(value))]"
}

func selectionFlag(role: String, selected: Bool?, value: Bool?) -> Bool? {
    // AccessKit 0.26.3 maps Tab -> AXRadioButton/AXValue; rows use AXSelected.
    role == "AXRadioButton" ? value : selected
}

func selection(_ node: Node) -> Bool? {
    selectionFlag(role: node.role, selected: flagValue(attribute(node.element, kAXSelectedAttribute)),
                  value: flagValue(attribute(node.element, kAXValueAttribute)))
}

func selectionDescription(_ node: Node) -> String {
    let box = node.frame.map(fmt) ?? "-"
    return "\(node.role)/\(node.subrole) \(node.name); AXSelected=\(rawAttribute(node.element, kAXSelectedAttribute)); AXValue=\(rawAttribute(node.element, kAXValueAttribute)); frame=[\(box)]"
}

func tabStatesSummary(_ pid: pid_t) -> String {
    guard let window = mainWindow(pid) else { return "window=absent; \(frontDescription())" }
    let tabs = walk(window)
        .filter { $0.role == "AXRadioButton" }
        .map { selectionDescription($0) }
    let windowState = "AXMain=\(rawAttribute(window, kAXMainAttribute)); AXFocused=\(rawAttribute(window, kAXFocusedAttribute)); \(frontDescription())"
    return "\(windowState); tabs=[\(tabs.joined(separator: " | "))]"
}

struct StableFlag {
    let wanted: Bool
    let seconds: Double
    var since: Double?

    mutating func observe(_ value: Bool?, at time: Double) -> Bool {
        guard value == wanted else { since = nil; return false }
        if since == nil { since = time }
        return time - since! >= seconds
    }
}

func point(of value: CFTypeRef?) -> CGPoint? {
    guard let value, CFGetTypeID(value) == AXValueGetTypeID() else { return nil }
    var point = CGPoint.zero
    return AXValueGetValue(value as! AXValue, .cgPoint, &point) ? point : nil
}

func size(of value: CFTypeRef?) -> CGSize? {
    guard let value, CFGetTypeID(value) == AXValueGetTypeID() else { return nil }
    var size = CGSize.zero
    return AXValueGetValue(value as! AXValue, .cgSize, &size) ? size : nil
}

func frame(of element: AXUIElement) -> CGRect? {
    guard let origin = point(of: attribute(element, kAXPositionAttribute)),
          let size = size(of: attribute(element, kAXSizeAttribute)) else { return nil }
    return CGRect(origin: origin, size: size)
}

func string(_ value: Any?) -> String? {
    value as? String
}

/// One element and its children, read in a single round trip.
func read(_ element: AXUIElement, depth: Int) -> (Node, [AXUIElement]) {
    var values: CFArray?
    let status = AXUIElementCopyMultipleAttributeValues(element, fetched, AXCopyMultipleAttributeOptions(rawValue: 0), &values)
    let list = (status == .success ? values as? [Any] : nil) ?? []
    func at(_ index: Int) -> Any? {
        guard index < list.count else { return nil }
        let item = list[index] as CFTypeRef
        // Missing attributes come back as AXValue errors, not nil.
        if CFGetTypeID(item) == AXValueGetTypeID(), AXValueGetType(item as! AXValue) == .axError { return nil }
        return list[index]
    }
    var rect: CGRect?
    if let origin = point(of: at(5) as CFTypeRef?), let size = size(of: at(6) as CFTypeRef?) {
        rect = CGRect(origin: origin, size: size)
    }
    let node = Node(
        element: element,
        role: string(at(0)) ?? "",
        subrole: string(at(1)) ?? "",
        title: string(at(2)) ?? "",
        label: string(at(3)) ?? "",
        value: string(at(4)),
        frame: rect,
        depth: depth
    )
    let children = (at(7) as? [AXUIElement]) ?? []
    return (node, children)
}

/// Every element under `root`, breadth first, up to a bound that keeps one
/// pass well under a second on a full conversation.
func walk(_ root: AXUIElement, limit: Int = 20000) -> [Node] {
    var nodes: [Node] = []
    var queue: [(AXUIElement, Int)] = [(root, 0)]
    var head = 0
    while head < queue.count && nodes.count < limit {
        let (element, depth) = queue[head]
        head += 1
        let (node, children) = read(element, depth: depth)
        nodes.append(node)
        for child in children { queue.append((child, depth + 1)) }
    }
    return nodes
}

func application(_ pid: pid_t) -> AXUIElement {
    let app = AXUIElementCreateApplication(pid)
    AXUIElementSetMessagingTimeout(app, 3)
    return app
}

/// The runner reaps the app only when it next polls, so a crashed app stays a
/// zombie that `kill(pid, 0)` still accepts. Treat a zombie as exited.
func processAlive(_ pid: pid_t) -> Bool {
    guard kill(pid, 0) == 0 else { return false }
    var info = kinfo_proc()
    var size = MemoryLayout<kinfo_proc>.stride
    var name: [Int32] = [CTL_KERN, KERN_PROC, KERN_PROC_PID, pid]
    guard sysctl(&name, 4, &info, &size, nil, 0) == 0, size > 0 else { return true }
    return info.kp_proc.p_stat != SZOMB
}

/// The development copy's document window: the standard window, else the first.
func mainWindow(_ pid: pid_t) -> AXUIElement? {
    let windows = (attribute(application(pid), kAXWindowsAttribute) as? [AXUIElement]) ?? []
    return windows.first { string(attribute($0, kAXSubroleAttribute)) == kAXStandardWindowSubrole } ?? windows.first
}

func requireWindow(_ pid: pid_t) -> AXUIElement {
    guard processAlive(pid) else { die("process \(pid) is not running") }
    guard let window = mainWindow(pid) else { die("process \(pid) has no window in the accessibility tree") }
    return window
}

/// Polls until `body` returns a value or the time runs out.
func poll<T>(_ timeout: Double, interval: Double = 0.1, _ body: () -> T?) -> T? {
    let deadline = Date().addingTimeInterval(timeout)
    repeat {
        if let value = body() { return value }
        if Date() >= deadline { break }
        RunLoop.current.run(until: Date().addingTimeInterval(interval))
    } while true
    return nil
}

func matches(_ pid: pid_t, _ selector: Selector) -> [Node] {
    guard let window = mainWindow(pid) else { return [] }
    return walk(window).filter { selector.matches(role: $0.role, names: $0.names) }
}

func waitFor(_ pid: pid_t, _ selector: Selector) -> Node {
    var last = "no matching control"
    var result: Node?
    var exited = false
    _ = poll(selector.timeout) { () -> Bool? in
        if !processAlive(pid) { exited = true; return true }
        let found = matches(pid, selector)
        last = "\(found.count) matching controls (need \(selector.nth))"
        if found.count >= selector.nth {
            result = found[selector.nth - 1]
            return true
        }
        return nil
    }
    if exited || !processAlive(pid) { die("the development copy (process \(pid)) exited") }
    if let node = result { return node }
    die("timed out after \(selector.timeout)s waiting for \(selector.describe); last observed: \(last)")
}

func waitSelected(_ pid: pid_t, _ selector: Selector, transition: String = "observing selection") {
    let started = Date()
    var trace = WaitTrace()
    var exited = false
    let found = poll(selector.timeout) { () -> Bool? in
        if !processAlive(pid) { exited = true; return true }
        let elapsed = Date().timeIntervalSince(started)
        let nodes = matches(pid, selector)
        guard nodes.count >= selector.nth else {
            trace.record(at: elapsed, state: "control absent")
            return nil
        }
        let node = nodes[selector.nth - 1]
        trace.record(at: elapsed, state: selectionDescription(node))
        return selection(node) == true ? true : nil
    }
    if exited || !processAlive(pid) {
        die("the development copy (process \(pid)) exited while waiting for selected \(selector.describe); \(transition); \(trace.summary)")
    }
    guard found != nil else {
        let elapsed = String(format: "%.2fs", Date().timeIntervalSince(started))
        die("timed out after \(selector.timeout)s (elapsed \(elapsed)) waiting for selected \(selector.describe); \(transition); last observed: \(trace.last); \(trace.summary); \(tabStatesSummary(pid))")
    }
    say("selected \(selector.describe); \(transition); \(trace.last)")
}

// MARK: - Window state

func setBool(_ element: AXUIElement, _ name: String, _ on: Bool) -> AXError {
    AXUIElementSetAttributeValue(element, name as CFString, (on ? kCFBooleanTrue : kCFBooleanFalse) as CFTypeRef)
}

func bool(_ element: AXUIElement, _ name: String) -> Bool {
    flagValue(attribute(element, name)) ?? false
}

/// Brings the development copy forward through accessibility, which works
/// from a terminal where activation requests can be declined.
func front(_ pid: pid_t) {
    let app = application(pid)
    _ = setBool(app, kAXFrontmostAttribute, true)
    if let window = mainWindow(pid) {
        _ = setBool(window, kAXMainAttribute, true)
        AXUIElementPerformAction(window, kAXRaiseAction as CFString)
    }
    let forward = poll(3) { NSWorkspace.shared.frontmostApplication?.processIdentifier == pid ? true : nil }
    if forward == nil {
        die("timed out after 3s waiting for process \(pid) to be frontmost; last observed: \(frontDescription()); refusing to send input", 3)
    }
}

/// The visible part of the main screen in accessibility coordinates (top left).
func visibleFrame() -> CGRect {
    guard let screen = NSScreen.main, let primary = NSScreen.screens.first else { return .zero }
    let visible = screen.visibleFrame
    return topLeftFrame(visible, primaryTop: primary.frame.maxY)
}

func windowList(_ options: CGWindowListOption = [.optionOnScreenOnly, .excludeDesktopElements], relativeTo: CGWindowID = kCGNullWindowID) -> [WindowInfo] {
    let list = (CGWindowListCopyWindowInfo(options, relativeTo) as? [[String: Any]]) ?? []
    return list.compactMap { info in
        guard let bounds = info[kCGWindowBounds as String] as! CFDictionary?,
              let rect = CGRect(dictionaryRepresentation: bounds),
              let number = info[kCGWindowNumber as String] as? NSNumber,
              let pid = info[kCGWindowOwnerPID as String] as? NSNumber else { return nil }
        return WindowInfo(number: number.intValue, pid: pid.int32Value,
                          owner: info[kCGWindowOwnerName as String] as? String ?? "unknown",
                          frame: rect, layer: (info[kCGWindowLayer as String] as? NSNumber)?.intValue ?? -1,
                          alpha: (info[kCGWindowAlpha as String] as? NSNumber)?.doubleValue ?? 1)
    }
}

func windowNumber(_ pid: pid_t) -> Int? {
    guard let window = mainWindow(pid), let rect = frame(of: window) else { return nil }
    return windowList([.optionAll]).first {
        $0.pid == pid && $0.layer == 0 && abs($0.frame.minX - rect.minX) < 2 &&
            abs($0.frame.minY - rect.minY) < 2 && abs($0.frame.width - rect.width) < 2 &&
            abs($0.frame.height - rect.height) < 2
    }?.number
}

/// AppKit asks WindowServer which window would receive mouseDown, including
/// another app or the menu bar. Rectangle order also includes click-through
/// overlays and is not a hit test (AX hit succeeded on the first CI run).
func mouseWindowNumber(_ point: CGPoint) -> Int {
    // A command-line helper has no NSApplication unless it creates one. Set up
    // AppKit's connection without activating the helper or making any window.
    NSApplication.shared.setActivationPolicy(.prohibited)
    guard let primary = NSScreen.screens.first else { return 0 }
    return NSWindow.windowNumber(at: flipScreenPoint(point, primaryTop: primary.frame.maxY), belowWindowWithWindowNumber: 0)
}

func windowAt(_ point: CGPoint) -> WindowInfo? {
    let number = mouseWindowNumber(point)
    guard number > 0, number <= Int(UInt32.max) else { return nil }
    // Do not discard the system's answer through desktop/layer filtering.
    return windowList([.optionIncludingWindow], relativeTo: CGWindowID(number)).first { $0.number == number }
}

func frontDescription() -> String {
    guard let app = NSWorkspace.shared.frontmostApplication else { return "frontmost=unknown frame=unknown" }
    let window = windowList().first { $0.pid == app.processIdentifier && $0.layer == 0 }
    return "frontmost=\(app.localizedName ?? "unknown") pid=\(app.processIdentifier) frame=[\(window.map { fmt($0.frame) } ?? "unknown")]"
}

func pointDescription(_ point: CGPoint) -> String {
    let rectangle = windowList().first { $0.alpha > 0 && $0.frame.contains(point) }
    return "point=[\(fmt(point.x)) \(fmt(point.y))]; mouse target number=\(mouseWindowNumber(point)): \(windowAt(point)?.description ?? "unknown"); first covering rectangle: \(rectangle?.description ?? "unknown"); \(frontDescription())"
}

func requireOwnPoint(_ pid: pid_t, _ point: CGPoint) {
    guard let rect = frame(of: requireWindow(pid)),
          permitsPointer(target: windowAt(point), pid: pid, window: windowNumber(pid), frame: rect, point: point) else {
        die("refused: the window under \(fmt(point.x)), \(fmt(point.y)) is not the development copy's; \(pointDescription(point))", 3)
    }
    var element: AXUIElement?
    let result = AXUIElementCopyElementAtPosition(AXUIElementCreateSystemWide(), Float(point.x), Float(point.y), &element)
    var owner: pid_t = 0
    if let element { AXUIElementGetPid(element, &owner) }
    guard result == .success && owner == pid else {
        die("refused: accessibility hit owner=\(owner); \(pointDescription(point))", 3)
    }
}

// MARK: - Input

func postKey(_ pid: pid_t, _ combo: Combo) throws {
    let source = CGEventSource(stateID: .hidSystemState)
    for down in [true, false] {
        guard let event = CGEvent(keyboardEventSource: source, virtualKey: combo.key, keyDown: down) else {
            throw UsageError("could not make a key event")
        }
        event.flags = combo.flags
        event.postToPid(pid)
        Thread.sleep(forTimeInterval: 0.02)
    }
}

func postText(_ pid: pid_t, _ text: String, delay: Double) {
    let source = CGEventSource(stateID: .hidSystemState)
    for units in typingUnits(text) {
        for down in [true, false] {
            guard let event = CGEvent(keyboardEventSource: source, virtualKey: 0, keyDown: down) else {
                die("could not make a key event")
            }
            event.flags = []
            units.withUnsafeBufferPointer { buffer in
                event.keyboardSetUnicodeString(stringLength: buffer.count, unicodeString: buffer.baseAddress)
            }
            event.postToPid(pid)
        }
        Thread.sleep(forTimeInterval: delay)
    }
}

/// Pointer events go to whatever window is under the point, so they are sent
/// only when that is the development copy's, and the pointer is put back.
func pointer(_ pid: pid_t, at points: [CGPoint], _ body: () -> Void) {
    for point in points {
        requireOwnPoint(pid, point)
    }
    let flipped = NSEvent.mouseLocation
    let primary = NSScreen.screens.first?.frame.maxY ?? 0
    let original = flipScreenPoint(flipped, primaryTop: primary)
    body()
    Thread.sleep(forTimeInterval: 0.1)
    CGEvent(mouseEventSource: nil, mouseType: .mouseMoved, mouseCursorPosition: original, mouseButton: .left)?
        .post(tap: .cghidEventTap)
}

func mouse(_ type: CGEventType, _ point: CGPoint, clicks: Int64 = 1) {
    guard let event = CGEvent(mouseEventSource: CGEventSource(stateID: .hidSystemState), mouseType: type,
                              mouseCursorPosition: point, mouseButton: .left) else { die("could not make a pointer event") }
    event.setIntegerValueField(.mouseEventClickState, value: clicks)
    event.post(tap: .cghidEventTap)
}

func drag(_ from: CGPoint, _ to: CGPoint) {
    mouse(.mouseMoved, from)
    Thread.sleep(forTimeInterval: 0.1)
    mouse(.leftMouseDown, from)
    Thread.sleep(forTimeInterval: 0.15)
    let steps = 20
    for step in 1...steps {
        let t = CGFloat(step) / CGFloat(steps)
        mouse(.leftMouseDragged, CGPoint(x: from.x + (to.x - from.x) * t, y: from.y + (to.y - from.y) * t))
        Thread.sleep(forTimeInterval: 0.016)
    }
    Thread.sleep(forTimeInterval: 0.1)
    mouse(.leftMouseUp, to)
}

func click(_ point: CGPoint, count: Int) {
    mouse(.mouseMoved, point)
    Thread.sleep(forTimeInterval: 0.1)
    for index in 1...count {
        mouse(.leftMouseDown, point, clicks: Int64(index))
        Thread.sleep(forTimeInterval: 0.03)
        mouse(.leftMouseUp, point, clicks: Int64(index))
        Thread.sleep(forTimeInterval: 0.08)
    }
}

// MARK: - Self-test of the pure parts

func selfTest() {
    var failures = 0
    func check(_ condition: Bool, _ what: String) {
        if !condition { failures += 1; print("self-test failed: \(what)") }
    }
    do {
        let (selector, extra, rest) = try parseSelector(["--role", "control", "--prefix", "Branch ", "--timeout", "2", "--text", "hi", "on"])
        check(selector.roles.contains("AXButton") && selector.roles.contains("AXRadioButton"), "control expands")
        check(selector.prefix == "Branch " && selector.timeout == 2, "prefix and timeout")
        check(extra["--text"] == "hi" && rest == ["on"], "extra and rest")
        check(selector.matches(role: "AXButton", names: ["Branch main"]), "prefix matches")
        check(!selector.matches(role: "AXStaticText", names: ["Branch main"]), "role filters")
        check(!selector.matches(role: "AXButton", names: ["Switch branch"]), "prefix rejects")
    } catch { check(false, "parse: \(error)") }
    do {
        let (selector, _, _) = try parseSelector(["--regex", "^[0-9]+ of 3$", "--contains", "of"])
        check(selector.matches(role: "AXGroup", names: ["", "2 of 3"]), "regex and contains")
        check(!selector.matches(role: "AXGroup", names: ["2 of 30"]), "regex anchors")
    } catch { check(false, "parse regex: \(error)") }
    check((try? parseSelector(["--regex", "("])) == nil, "invalid regex refused")
    check((try? parseSelector(["--bogus"])) == nil, "unknown option refused")
    check((try? parseSelector(["--nth", "0"])) == nil, "nth must be positive")
    check((try? parseCombo("cmd+shift+2")) == Combo(key: 19, flags: [.maskCommand, .maskShift]), "cmd+shift+2")
    check((try? parseCombo("cmd+opt+period")) == Combo(key: 47, flags: [.maskCommand, .maskAlternate]), "cmd+opt+period")
    check((try? parseCombo("escape")) == Combo(key: 53, flags: []), "escape")
    check((try? parseCombo("cmd+")) == nil, "a combo needs a key")
    check((try? parseCombo("cmd+a+b")) == nil, "one key only")
    check(typingUnits("aé🙂").map(\.count) == [1, 1, 2], "one event per character, surrogate pairs kept")
    let done = Selector(roles: expandRoles("control"), exact: "Done")
    check(done.matches(role: "AXButton", names: ["Done"]), "Done settings button matches")
    check(!done.matches(role: "AXStaticText", names: ["Done"]), "Done thread status is not a button")
    check(!done.matches(role: "AXRow", names: ["Thread 123 · Done · fixture"]), "Done must be the whole name")
    check(Selector().timeout == 3, "ordinary AX waits are bounded to three seconds")
    check(flagValue(kCFBooleanTrue) == true && flagValue(kCFBooleanFalse) == false, "CFBoolean flags")
    check(flagValue(NSNumber(value: 1)) == true && flagValue(NSNumber(value: 0)) == false, "numeric flags")
    check(flagValue(nil) == nil && flagValue("1" as CFString) == nil && flagValue(NSNumber(value: 2)) == nil, "missing, text and mixed values stay unknown")
    check(selectionFlag(role: "AXRadioButton", selected: false, value: true) == true, "tabs use AXValue")
    check(selectionFlag(role: "AXRadioButton", selected: true, value: false) == false, "tab AXSelected cannot override AXValue")
    check(selectionFlag(role: "AXRow", selected: false, value: true) == false, "rows use AXSelected")
    check(selectionFlag(role: "AXRow", selected: nil, value: true) == nil, "missing row selection is unknown, not false")
    var stable = StableFlag(wanted: true, seconds: 1.5)
    check(!stable.observe(true, at: 0) && !stable.observe(false, at: 1), "transient full screen flag is not completion")
    check(!stable.observe(true, at: 2) && !stable.observe(nil, at: 3), "missing window clears stability")
    check(!stable.observe(true, at: 4) && !stable.observe(true, at: 5) && stable.observe(true, at: 5.5), "full screen must remain true for the complete interval")
    let point = CGPoint(x: 177.5, y: 73)
    check(flipScreenPoint(point, primaryTop: 768) == CGPoint(x: 177.5, y: 695), "AX point to AppKit, no pixel scaling")
    check(flipScreenPoint(flipScreenPoint(point, primaryTop: 768), primaryTop: 768) == point, "screen conversion is its own inverse")
    check(topLeftFrame(CGRect(x: -1200, y: 800, width: 1200, height: 900), primaryTop: 768) == CGRect(x: -1200, y: -932, width: 1200, height: 900), "display above and left of primary")
    let visible = CGRect(x: 0, y: 31, width: 1024, height: 679)
    let rect = placement(width: 1380, height: 860, visible: visible)
    check(rect == CGRect(x: 20, y: 51, width: 984, height: 639), "first runner geometry including menu bar and margins")
    let own = WindowInfo(number: 8, pid: 42, owner: "fixture", frame: rect, layer: 0, alpha: 1)
    let other = WindowInfo(number: 9, pid: 43, owner: "other", frame: rect, layer: 0, alpha: 1)
    let sameApp = WindowInfo(number: 10, pid: 42, owner: "fixture", frame: rect, layer: 0, alpha: 1)
    let dock = WindowInfo(number: 6, pid: 331, owner: "Dock", frame: CGRect(x: 0, y: 0, width: 1024, height: 768), layer: 20, alpha: 1)
    check(permitsPointer(target: own, pid: 42, window: 8, frame: rect, point: point), "own mouse target, even under a click-through covering rectangle")
    check(!permitsPointer(target: other, pid: 42, window: 8, frame: rect, point: point), "another app in front refused")
    check(!permitsPointer(target: dock, pid: 42, window: 8, frame: rect, point: point), "Dock is refused if it actually receives the mouse event")
    check(!permitsPointer(target: sameApp, pid: 42, window: 8, frame: rect, point: point), "another window of same PID refused")
    check(!permitsPointer(target: own, pid: 42, window: 8, frame: rect, point: CGPoint(x: 177.5, y: 15)), "menu bar point refused")
    check(!permitsPointer(target: nil, pid: 42, window: 8, frame: rect, point: point), "unknown mouse target refused")
    check(!permitsPointer(target: own, pid: 42, window: nil, frame: rect, point: point), "unknown own window refused")
    check(messageSelectionSpan(box: CGRect(x: 300, y: 200, width: 500, height: 30)) == nil, "message shorter than header plus line is rejected")
    if let (from, to) = messageSelectionSpan(box: CGRect(x: 300, y: 200, width: 500, height: 47)) {
        check(from == CGPoint(x: 303, y: 236) && to == CGPoint(x: 460, y: 236), "message drag clears author header line")
    } else { check(false, "selectable message frame accepted") }
    // Recorded 1-tab CI tree (984x639 at 20,51): New thread tab ends at 408, window right at 1004.
    let ciWindow = CGRect(x: 20, y: 51, width: 984, height: 639)
    let secondRow = [
        BarElement(role: "AXRadioButton", frame: CGRect(x: 668, y: 96, width: 93, height: 24)),
        BarElement(role: "AXButton", frame: CGRect(x: 964, y: 94, width: 28, height: 28)),
    ]
    let oneTabBar = [
        BarElement(role: "AXButton", frame: CGRect(x: 33, y: 62, width: 16, height: 16)),
        BarElement(role: "AXButton", frame: CGRect(x: 79, y: 62, width: 16, height: 16)),
        BarElement(role: "AXButton", frame: CGRect(x: 104, y: 56, width: 28, height: 28)),
        BarElement(role: "AXTabGroup", frame: CGRect(x: 136, y: 51, width: 240, height: 39)),
        BarElement(role: "AXRadioButton", frame: CGRect(x: 136, y: 56, width: 240, height: 34)),
        BarElement(role: "AXButton", frame: CGRect(x: 380, y: 56, width: 28, height: 28)),
    ] + secondRow
    check(tabBarDragPoint(window: ciWindow, elements: oneTabBar) == .point(CGPoint(x: 706, y: 71), left: 416, right: 996), "1-tab CI tree uses empty right end centered on 40-point bar and ignores second row")
    // Recorded 4-tab full-row CI tree (turn-redo-ax.txt): Open tabs 136..924, New thread tab 928..956.
    let fullRowBar = [
        BarElement(role: "AXButton", frame: CGRect(x: 104, y: 56, width: 28, height: 28)),
        BarElement(role: "AXTabGroup", frame: CGRect(x: 136, y: 51, width: 788, height: 39)),
        BarElement(role: "AXButton", frame: CGRect(x: 928, y: 56, width: 28, height: 28)),
    ] + secondRow
    check(tabBarDragPoint(window: ciWindow, elements: fullRowBar) == .point(CGPoint(x: 980, y: 71), left: 964, right: 996), "full-row CI tree uses remaining 48-point right end")
    // Recorded wide window (editor-page-custom-state-ax.txt): 1790x1129 at 10,39; New thread tab 370..398.
    let wideWindow = CGRect(x: 10, y: 39, width: 1790, height: 1129)
    let wideBar = [
        BarElement(role: "AXButton", frame: CGRect(x: 94, y: 44.5, width: 28, height: 28)),
        BarElement(role: "AXTabGroup", frame: CGRect(x: 126, y: 39, width: 240, height: 39)),
        BarElement(role: "AXButton", frame: CGRect(x: 370, y: 44.5, width: 28, height: 28)),
    ]
    check(tabBarDragPoint(window: wideWindow, elements: wideBar) == .point(CGPoint(x: 1099, y: 59), left: 406, right: 1792), "wide window centers drag point in empty right end")
    // Full row with overflow button leaving no empty right end -> noEmptyEnd (skip, not fail).
    let noRoomBar = [
        BarElement(role: "AXTabGroup", frame: CGRect(x: 136, y: 51, width: 720, height: 39)),
        BarElement(role: "AXButton", frame: CGRect(x: 860, y: 56, width: 104, height: 28)),
        BarElement(role: "AXButton", frame: CGRect(x: 968, y: 56, width: 28, height: 28)),
    ]
    check(tabBarDragPoint(window: ciWindow, elements: noRoomBar) == .noEmptyEnd(rightmost: 996, windowRight: 1004, available: 8), "crowded tab bar reports no empty end instead of returning a point")
    check(tabBarDragPoint(window: ciWindow, elements: secondRow) == .noControls, "missing top-bar controls reported separately from crowded bar")
    // Unedited excerpts of recorded trees: the filter must reject the light
    // group, sidebar, splitter and second row on its own.
    let recordedOneTab =     """
    AXWindow/AXStandardWindow title="Plait" label="" value="" frame=20.0 51.0 984.0 639.0
      AXFullScreen=false(0); AXMinimized=false(0)
      AXGroup/AXUnknown title="" label="" value="" frame=20.0 51.0 984.0 639.0
      AXButton/AXCloseButton title="" label="" value="" frame=33.0 62.0 16.0 16.0
      AXButton/AXFullScreenButton title="" label="" value="" frame=79.0 62.0 16.0 16.0
      AXButton/AXMinimizeButton title="" label="" value="" frame=56.0 62.0 16.0 16.0
        AXGroup/AXUnknown title="Plait" label="" value="" frame=20.0 51.0 984.0 639.0
        AXGroup title="" label="" value="" frame=80.0 63.0 14.0 14.0
          AXButton/AXUnknown title="Toggle sidebar" label="" value="" frame=104.0 56.0 28.0 28.0
          AXTabGroup/AXUnknown title="Open tabs" label="" value="" frame=136.0 51.0 240.0 39.0
          AXButton/AXUnknown title="New thread tab" label="" value="" frame=380.0 56.0 28.0 28.0
          AXButton/AXUnknown title="New thread: choose a model…" label="" value="" frame=28.0 97.0 194.0 28.0
          AXButton/AXUnknown title="New thread with another model…" label="" value="" frame=223.0 97.0 28.0 28.0
          AXButton/AXUnknown title="Open repository…" label="" value="" frame=28.0 126.0 223.0 28.0
          AXTextField/AXSearchField title="Filter projects and threads" label="" value="" frame=28.0 166.0 223.0 28.0
          AXCheckBox/AXToggle title="Show only threads needing attention" label="" value="" frame=28.0 203.0 73.0 22.0
          AXCheckBox/AXToggle title="Include archived threads" label="" value="" frame=105.0 203.0 67.0 22.0
          AXButton/AXUnknown title="Alerts on: mute notifications and attention requests" label="" value="" frame=199.0 202.0 24.0 24.0
          AXButton/AXUnknown title="Refresh threads and folders" label="" value="" frame=227.0 202.0 24.0 24.0
          AXOutline/AXUnknown title="Projects, machines, and threads" label="" value="" frame=20.0 226.0 239.0 424.0
          AXStaticText/AXUnknown title="" label="" value="Local host connected" frame=55.0 661.0 113.0 18.0
          AXButton/AXUnknown title="Update available. Open Settings to install." label="" value="" frame=184.0 656.0 68.0 28.0
          AXButton/AXUnknown title="Settings…" label="" value="" frame=256.0 656.0 28.0 28.0
          AXSplitter/AXUnknown title="Resize sidebar" label="" value="" frame=253.0 91.0 6.0 599.0
          AXRadioButton/AXTabButton title="Conversation" label="" value="" frame=668.0 96.0 93.0 24.0
            state: AXRadioButton/AXTabButton Conversation; AXSelected=unavailable(AXError=-25212); AXValue=true(1)
          AXRadioButton/AXTabButton title="Changes" label="" value="" frame=763.0 96.0 82.0 24.0
            state: AXRadioButton/AXTabButton Changes; AXSelected=unavailable(AXError=-25212); AXValue=false(0)
          AXRadioButton/AXTabButton title="History" label="" value="" frame=847.0 96.0 59.0 24.0
            state: AXRadioButton/AXTabButton History; AXSelected=unavailable(AXError=-25212); AXValue=false(0)
          AXRadioButton/AXTabButton title="Files" label="" value="" frame=908.0 96.0 45.0 24.0
            state: AXRadioButton/AXTabButton Files; AXSelected=unavailable(AXError=-25212); AXValue=false(0)
          AXButton/AXUnknown title="Git inspector (needs a wider window)" label="" value="" frame=964.0 94.0 28.0 28.0
    """
    let recordedFullRow =     """
    AXWindow/AXStandardWindow title="Plait" label="" value="" frame=20.0 51.0 984.0 639.0
      AXFullScreen=false(0); AXMinimized=false(0)
      AXGroup/AXUnknown title="" label="" value="" frame=20.0 51.0 984.0 639.0
      AXButton/AXCloseButton title="" label="" value="" frame=33.0 62.0 16.0 16.0
      AXButton/AXFullScreenButton title="" label="" value="" frame=79.0 62.0 16.0 16.0
      AXButton/AXMinimizeButton title="" label="" value="" frame=56.0 62.0 16.0 16.0
        AXGroup/AXUnknown title="Plait" label="" value="" frame=20.0 51.0 984.0 639.0
        AXGroup title="" label="" value="" frame=80.0 63.0 14.0 14.0
          AXButton/AXUnknown title="Toggle sidebar" label="" value="" frame=104.0 56.0 28.0 28.0
          AXTabGroup/AXUnknown title="Open tabs" label="" value="" frame=136.0 51.0 788.0 39.0
          AXButton/AXUnknown title="New thread tab" label="" value="" frame=928.0 56.0 28.0 28.0
    """
    let recordedWide =     """
    AXWindow/AXStandardWindow title="Plait" label="" value="" frame=10.0 39.0 1790.0 1129.0
      AXFullScreen=false(0); AXMinimized=false(0)
      AXGroup/AXUnknown title="" label="" value="" frame=10.0 39.0 1790.0 1129.0
      AXButton/AXCloseButton title="" label="" value="" frame=23.0 50.0 16.0 16.0
      AXButton/AXFullScreenButton title="" label="" value="" frame=69.0 50.0 16.0 16.0
      AXButton/AXMinimizeButton title="" label="" value="" frame=46.0 50.0 16.0 16.0
        AXGroup/AXUnknown title="Plait" label="" value="" frame=10.0 39.0 1790.0 1129.0
        AXGroup title="" label="" value="" frame=70.0 51.0 14.0 14.0
          AXButton/AXUnknown title="Toggle sidebar" label="" value="" frame=94.0 44.5 28.0 28.0
          AXTabGroup/AXUnknown title="Open tabs" label="" value="" frame=126.0 39.0 240.0 39.0
          AXButton/AXUnknown title="New thread tab" label="" value="" frame=370.0 44.5 28.0 28.0
    """
    let recorded: [(String, String, TabBarEnd)] = [
        ("one tab (CI)", recordedOneTab, .point(CGPoint(x: 706, y: 71), left: 416, right: 996)),
        ("full row (CI)", recordedFullRow, .point(CGPoint(x: 980, y: 71), left: 964, right: 996)),
        ("wide window", recordedWide, .point(CGPoint(x: 1099, y: 59), left: 406, right: 1792)),
    ]
    for (what, dump, expected) in recorded {
        if let (window, elements) = barElements(dump: dump) {
            check(elements.count > 4 && tabBarDragPoint(window: window, elements: elements) == expected, "recorded \(what) tree drag point")
        } else { check(false, "recorded \(what) tree parsed") }
    }
    // Verify selection decoding against recorded CI dump states (AXRadioButton tabs and AXRow threads).
    let recordedStates: [(role: String, selected: Bool?, value: Bool?, expected: Bool?)] = [
        ("AXRadioButton", nil, true, true),    // Conversation AXSelected=unavailable, AXValue=true(1)
        ("AXRadioButton", nil, false, false),  // Files AXSelected=unavailable, AXValue=false(0)
        ("AXRow", true, nil, true),            // Thread AXRow AXSelected=true(1), AXValue=unavailable
        ("AXRow", false, nil, false),          // Project AXRow AXSelected=false(0), AXValue=unavailable
    ]
    for sample in recordedStates {
        check(selectionFlag(role: sample.role, selected: sample.selected, value: sample.value) == sample.expected,
              "recorded state selection for \(sample.role)")
    }
    var trace = WaitTrace()
    trace.record(at: 0.0, state: "AXRadioButton Files AXValue=false(0)")
    trace.record(at: 0.1, state: "AXRadioButton Files AXValue=false(0)")
    trace.record(at: 0.2, state: "AXRadioButton Files AXValue=true(1)")
    check(trace.polls == 3 && trace.events.count == 2 && trace.summary.contains("polls=3"), "wait trace deduplicates repeated states")
    var growth = GrowthTrace()
    growth.observePresent(at: 1.2, length: 30, role: "AXGroup", frame: "284 468 696 47")
    growth.observePresent(at: 1.25, length: 30, role: "AXGroup", frame: "284 468 696 47")
    check(growth.observeAbsent(at: 8.05) && !growth.grew, "early disappearance stops growth tracking")
    check(growth.failureMessage(timeout: 15, describe: "writing").contains("disappeared at 8.05s before growing (not a timeout"), "disappearance is distinguished from timeout")
    var growing = GrowthTrace()
    growing.observePresent(at: 0.5, length: 30, role: "AXGroup", frame: "0 0 100 20")
    growing.observePresent(at: 0.65, length: 42, role: "AXGroup", frame: "0 0 100 20")
    check(growing.grew && growing.lengths == [30, 42], "two distinct lengths satisfy growth")
    check(processAlive(getpid()), "own process is alive")
    // An exited child that nobody has reaped yet, as the app is before the runner polls.
    var child: pid_t = 0
    if posix_spawn(&child, "/usr/bin/true", nil, nil, [strdup("true"), nil], nil) == 0 {
        let deadline = Date().addingTimeInterval(5)
        while processAlive(child) && Date() < deadline { usleep(20_000) }
        check(kill(child, 0) == 0 && !processAlive(child), "unreaped exited child counts as exited")
        var status: Int32 = 0
        waitpid(child, &status, 0)
    } else { check(false, "spawn /usr/bin/true for the exit test") }
    if failures > 0 { exit(1) }
    print("axdrive self-test passed")
}

// MARK: - Commands

let usage = """
usage: axdrive self-test | preflight
       axdrive COMMAND PID [selector] [arguments]
commands with a PID:
  wait-window, window, window-id, windows, place W H, lights, front, dump,
  tab-states, tab-bar-end, find, frame, absent, count, press, select,
  wait-selected, focus, focused, selected, value, set-value --text T,
  growth, key COMBO, type --text T [--delay S], fullscreen on|off,
  minimized on|off|get, hit X Y,
  drag X1 Y1 X2 Y2, double-click X Y, select-text --text T
selector: --role R --name N --prefix P --contains C --regex E --nth N --timeout S
"""

func number(_ text: String) -> CGFloat {
    guard let value = Double(text) else { die("not a number: \(text)\n\(usage)", 2) }
    return CGFloat(value)
}

func run(_ arguments: [String]) throws {
    guard let command = arguments.first else { die(usage, 2) }
    switch command {
    case "self-test":
        selfTest()
        return
    case "preflight":
        let trusted = AXIsProcessTrusted()
        say("accessibility \(trusted ? "yes" : "no")")
        say("screen-recording \(CGPreflightScreenCaptureAccess() ? "yes" : "no")")
        if !trusted { exit(2) }
        return
    default:
        break
    }
    guard arguments.count >= 2, let pid = pid_t(arguments[1]), pid > 0 else { die(usage, 2) }
    let (selector, extra, rest) = try parseSelector(Array(arguments.dropFirst(2)))
    switch command {
    case "wait-window":
        guard let window = poll(selector.timeout, { mainWindow(pid) }), let rect = frame(of: window) else {
            die("timed out after \(selector.timeout)s waiting for a development window with a frame; last observed: \(processAlive(pid) ? "process alive, no usable window" : "process exited")")
        }
        say(fmt(rect))
    case "window":
        let window = requireWindow(pid)
        guard let rect = frame(of: window) else { die("window has no frame") }
        guard let full = flagValue(attribute(window, "AXFullScreen")),
              let minimized = flagValue(attribute(window, kAXMinimizedAttribute)) else {
            die("window state unavailable: AXFullScreen=\(rawAttribute(window, "AXFullScreen")); AXMinimized=\(rawAttribute(window, kAXMinimizedAttribute))")
        }
        say("\(fmt(rect)) \(full ? 1 : 0) \(minimized ? 1 : 0)")
    case "window-id":
        guard let number = windowNumber(pid) else { die("no on-screen window for process \(pid)") }
        say(String(number))
    case "windows":
        say(frontDescription())
        for info in windowList() { say(info.description) }
    case "place":
        guard rest.count == 2 else { die(usage, 2) }
        let window = requireWindow(pid)
        let visible = visibleFrame()
        let target = placement(width: number(rest[0]), height: number(rest[1]), visible: visible)
        var origin = target.origin
        var size = target.size
        let moved = AXUIElementSetAttributeValue(window, kAXPositionAttribute as CFString, AXValueCreate(.cgPoint, &origin)!)
        let sized = AXUIElementSetAttributeValue(window, kAXSizeAttribute as CFString, AXValueCreate(.cgSize, &size)!)
        guard moved == .success && sized == .success else { die("could not place own window: position=\(moved.rawValue) size=\(sized.rawValue)") }
        Thread.sleep(forTimeInterval: 0.5)
        guard let rect = frame(of: window) else { die("window has no frame") }
        say("\(fmt(rect)) screen \(fmt(visible.width)) \(fmt(visible.height))")
    case "lights":
        let window = requireWindow(pid)
        guard let rect = frame(of: window) else { die("window has no frame") }
        var line = fmt(rect)
        for name in [kAXCloseButtonAttribute, kAXMinimizeButtonAttribute, kAXZoomButtonAttribute] {
            guard let button = attribute(window, name), CFGetTypeID(button) == AXUIElementGetTypeID(),
                  let buttonFrame = frame(of: button as! AXUIElement) else { die("window has no \(name)") }
            line += " " + fmt(buttonFrame)
        }
        say(line)
    case "front":
        front(pid)
    case "dump":
        for node in walk(requireWindow(pid)) {
            let indent = String(repeating: "  ", count: min(node.depth, 40))
            let value = (node.value ?? "").prefix(120).replacingOccurrences(of: "\n", with: "\\n")
            say("\(indent)\(node.role)\(node.subrole.isEmpty ? "" : "/" + node.subrole) title=\"\(node.title)\" label=\"\(node.label.prefix(120))\" value=\"\(value)\" frame=\(node.frame.map(fmt) ?? "-")")
            if ["AXRadioButton", "AXRow"].contains(node.role) { say("\(indent)  state: \(selectionDescription(node))") }
            if node.role == "AXWindow" { say("\(indent)  AXFullScreen=\(rawAttribute(node.element, "AXFullScreen")); AXMinimized=\(rawAttribute(node.element, kAXMinimizedAttribute))") }
        }
    case "tab-states":
        _ = requireWindow(pid)
        say(tabStatesSummary(pid))
    case "tab-bar-end":
        let window = requireWindow(pid)
        guard let rect = frame(of: window) else { die("window has no frame") }
        var elements = walk(window).compactMap { node -> BarElement? in
            node.frame.map { BarElement(role: node.role, frame: $0) }
        }
        for name in [kAXCloseButtonAttribute, kAXMinimizeButtonAttribute, kAXZoomButtonAttribute] {
            if let button = attribute(window, name), CFGetTypeID(button) == AXUIElementGetTypeID(),
               let box = frame(of: button as! AXUIElement) {
                elements.append(BarElement(role: "AXButton", frame: box))
            }
        }
        switch tabBarDragPoint(window: rect, elements: elements) {
        case let .point(pt, _, _):
            say("\(fmt(pt.x)) \(fmt(pt.y))")
        case let .noEmptyEnd(rightmost, windowRight, available):
            // The app always reserves at least 48 points here (even with a full
            // row), so a missing end is a lost drag area, not a reason to skip.
            die("no empty right end in the 40-point tab row (the app reserves at least 48 pt): rightmost control ends at \(fmt(rightmost)), window right edge is \(fmt(windowRight)) (\(fmt(available)) pt remaining)")
        case .noControls:
            die("no tab-row controls found in the 40-point bar", 1)
        }
    case "find":
        let node = waitFor(pid, selector)
        say("\(node.role)\t\(node.name.prefix(200))\t\(node.frame.map(fmt) ?? "-")")
    case "frame":
        let node = waitFor(pid, selector)
        guard let rect = node.frame else { die("\(selector.describe) has no frame") }
        say(fmt(rect))
    case "count":
        say(String(matches(pid, selector).count))
    case "absent":
        var last = "control still present"
        var exited = false
        let gone = poll(selector.timeout) { () -> Bool? in
            if !processAlive(pid) { exited = true; return true }
            let found = matches(pid, selector)
            last = "\(found.count) matching controls remain"
            return found.isEmpty ? true : nil
        }
        if exited || !processAlive(pid) { die("the development copy (process \(pid)) exited") }
        if gone == nil { die("timed out after \(selector.timeout)s waiting for absence of \(selector.describe); last observed: \(last)") }
    case "press":
        let node = waitFor(pid, selector)
        let result = AXUIElementPerformAction(node.element, kAXPressAction as CFString)
        guard result == .success else { die("press failed (\(result.rawValue)) on \(selector.describe)") }
        say("pressed \(node.role) \(node.name.prefix(120))")
    case "select":
        let node = waitFor(pid, selector)
        let before = selectionDescription(node)
        var action = "already selected; no press"
        if selection(node) != true {
            let result = AXUIElementPerformAction(node.element, kAXPressAction as CFString)
            guard result == .success else { die("press failed (\(result.rawValue)) on \(selector.describe); \(selectionDescription(node))") }
            action = "AXPress succeeded"
        }
        waitSelected(pid, selector, transition: "before: \(before); \(action)")
    case "wait-selected":
        waitSelected(pid, selector)
    case "focus":
        let node = waitFor(pid, selector)
        let result = setBool(node.element, kAXFocusedAttribute, true)
        guard result == .success else { die("focus failed (\(result.rawValue)) on \(selector.describe)") }
    case "focused":
        say(bool(waitFor(pid, selector).element, kAXFocusedAttribute) ? "1" : "0")
    case "selected":
        let node = waitFor(pid, selector)
        guard let selected = selection(node) else { die("selection unavailable: \(selectionDescription(node))") }
        say(selected ? "1" : "0")
    case "value":
        let node = waitFor(pid, selector)
        print(string(attribute(node.element, kAXValueAttribute)) ?? "", terminator: "")
    case "set-value":
        let node = waitFor(pid, selector)
        let result = AXUIElementSetAttributeValue(node.element, kAXValueAttribute as CFString, (extra["--text"] ?? "") as CFString)
        guard result == .success else { die("set value failed (\(result.rawValue)) on \(selector.describe)") }
    case "growth":
        // Watches a live element's name while it grows, as streaming text does.
        let started = Date()
        var trace = GrowthTrace()
        let deadline = started.addingTimeInterval(selector.timeout)
        while Date() < deadline {
            if !processAlive(pid) {
                die("the development copy (process \(pid)) exited while waiting for growth of \(selector.describe); observed=[\(trace.sampleDescription)]")
            }
            let elapsed = Date().timeIntervalSince(started)
            let found = matches(pid, selector)
            if let node = found.first {
                trace.observePresent(at: elapsed, length: node.name.count, role: node.role, frame: node.frame.map(fmt) ?? "-")
            } else if trace.observeAbsent(at: elapsed) {
                break
            }
            RunLoop.current.run(until: Date().addingTimeInterval(0.05))
        }
        guard trace.grew else {
            die(trace.failureMessage(timeout: selector.timeout, describe: selector.describe))
        }
        let first = trace.firstSeenAt.map { String(format: "%.2fs", $0) } ?? "?"
        say("\(trace.lengths.count) different lengths, longest \(trace.lengths.max() ?? 0) characters (first at \(first), polls=\(trace.polls))")
    case "key":
        guard let text = rest.first else { die(usage, 2) }
        let combo = try parseCombo(text)
        front(pid)
        try postKey(pid, combo)
    case "type":
        guard let text = extra["--text"] else { die(usage, 2) }
        front(pid)
        postText(pid, text, delay: Double(extra["--delay"] ?? "0.008") ?? 0.008)
    case "fullscreen":
        guard let state = rest.first, state == "on" || state == "off" else { die(usage, 2) }
        front(pid)
        let window = requireWindow(pid)
        let wanted = state == "on"
        let before = rawAttribute(window, "AXFullScreen")
        if flagValue(attribute(window, "AXFullScreen")) != wanted {
            let result = setBool(window, "AXFullScreen", wanted)
            guard result == .success else { die("AXFullScreen could not be set (\(result.rawValue)); before=\(before)") }
        }
        var stable = StableFlag(wanted: wanted, seconds: 1.5)
        var last = before
        var exited = false
        let settled = poll(selector.timeout, interval: 0.1) { () -> Bool? in
            if !processAlive(pid) { exited = true; return true }
            // The setter's original AX object may report the requested value
            // before the Space transition settles. Read a fresh window each time.
            guard let current = mainWindow(pid) else {
                last = "window absent"
                _ = stable.observe(nil, at: ProcessInfo.processInfo.systemUptime)
                return nil
            }
            last = "AXFullScreen=\(rawAttribute(current, "AXFullScreen")); frame=\(frame(of: current).map(fmt) ?? "unknown")"
            return stable.observe(flagValue(attribute(current, "AXFullScreen")), at: ProcessInfo.processInfo.systemUptime) ? true : nil
        }
        if exited || !processAlive(pid) { die("the development copy (process \(pid)) exited") }
        guard settled != nil else { die("timed out waiting for stable full screen \(state) for 1.5s; before=\(before); last observed: \(last)") }
        say("stable full screen \(state); before=\(before); \(last)")
    case "minimized":
        guard let state = rest.first else { die(usage, 2) }
        let window = (attribute(application(pid), kAXWindowsAttribute) as? [AXUIElement])?.first ?? requireWindow(pid)
        if state == "get" {
            say(bool(window, kAXMinimizedAttribute) ? "1" : "0")
        } else {
            _ = setBool(window, kAXMinimizedAttribute, state == "on")
            Thread.sleep(forTimeInterval: 1)
        }
    case "hit":
        guard rest.count == 2 else { die(usage, 2) }
        front(pid)
        requireOwnPoint(pid, CGPoint(x: number(rest[0]), y: number(rest[1])))
        var element: AXUIElement?
        let result = AXUIElementCopyElementAtPosition(AXUIElementCreateSystemWide(), Float(number(rest[0])), Float(number(rest[1])), &element)
        guard result == .success, let element else { die("nothing at that point") }
        var owner: pid_t = 0
        AXUIElementGetPid(element, &owner)
        // Check ownership before reading a name that could belong to another app.
        guard owner == pid else { die("refused: accessibility hit owner=\(owner); \(pointDescription(CGPoint(x: number(rest[0]), y: number(rest[1]))))", 3) }
        let (node, _) = read(element, depth: 0)
        say("owner \(owner) \(node.role) \(node.name.prefix(80))")
        if expandRoles("control,field").contains(node.role) { exit(1) }
    case "drag":
        guard rest.count == 4 else { die(usage, 2) }
        let from = CGPoint(x: number(rest[0]), y: number(rest[1]))
        let to = CGPoint(x: number(rest[2]), y: number(rest[3]))
        front(pid)
        pointer(pid, at: [from]) { drag(from, to) }
    case "select-text":
        guard let expected = extra["--text"], !expected.isEmpty else { die(usage, 2) }
        let node = waitFor(pid, selector)
        guard let box = node.frame, let (from, to) = messageSelectionSpan(box: box) else {
            die("message has no selectable frame")
        }
        // Keep every clipboard representation in memory; none enters artifacts.
        let board = NSPasteboard.general
        let saved = (board.pasteboardItems ?? []).map { item in
            item.types.compactMap { type in item.data(forType: type).map { (type, $0) } }
        }
        defer {
            board.clearContents()
            let items = saved.map { values -> NSPasteboardItem in
                let item = NSPasteboardItem()
                for (type, data) in values { item.setData(data, forType: type) }
                return item
            }
            board.writeObjects(items)
        }
        front(pid)
        pointer(pid, at: [from, to]) { drag(from, to) }
        board.clearContents()
        try postKey(pid, parseCombo("cmd+c"))
        guard let copied = poll(2, { board.string(forType: .string) }),
              copied.count >= 3, expected.contains(copied), copied != expected else {
            // Unwind through the clipboard restore even when the check fails.
            throw UsageError("drag and Cmd-C did not copy a proper substring of the fixture message")
        }
        say("selected and copied \(copied.count) fixture characters; clipboard restored")
    case "double-click":
        guard rest.count == 2 else { die(usage, 2) }
        let at = CGPoint(x: number(rest[0]), y: number(rest[1]))
        front(pid)
        pointer(pid, at: [at]) { click(at, count: 2) }
    default:
        die(usage, 2)
    }
}

/// First-line drag span inside a message AXGroup, skipping the 22-point author
/// header line and 4-point gap above the message text.
func messageSelectionSpan(box: CGRect) -> (CGPoint, CGPoint)? {
    let textTop: CGFloat = 26
    guard box.width > 80, box.height >= textTop + 12 else { return nil }
    let y = min(box.maxY - 6, box.minY + textTop + 10)
    return (
        CGPoint(x: box.minX + 3, y: y),
        CGPoint(x: min(box.maxX - 3, box.minX + 160), y: y)
    )
}

do {
    try run(Array(CommandLine.arguments.dropFirst()))
} catch {
    die("\(error)\n\(usage)", 2)
}
