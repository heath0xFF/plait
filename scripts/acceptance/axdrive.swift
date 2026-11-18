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
    var timeout: Double = 5

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

// MARK: - Finding a free part of the window bar (pure, covered by self-test)

/// The middle of the widest stretch of [from, to] no element covers, if it is
/// at least `minimum` wide.
func widestGap(occupied: [(CGFloat, CGFloat)], from: CGFloat, to: CGFloat, minimum: CGFloat) -> CGFloat? {
    let spans = occupied
        .map { (max($0.0, from), min($0.1, to)) }
        .filter { $0.0 < $0.1 }
        .sorted { $0.0 < $1.0 }
    var best: (CGFloat, CGFloat)?
    var cursor = from
    for span in spans {
        if span.0 > cursor, best == nil || span.0 - cursor > best!.1 - best!.0 {
            best = (cursor, span.0)
        }
        cursor = max(cursor, span.1)
    }
    if to > cursor, best == nil || to - cursor > best!.1 - best!.0 {
        best = (cursor, to)
    }
    guard let best, best.1 - best.0 >= minimum else { return nil }
    return (best.0 + best.1) / 2
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

func processAlive(_ pid: pid_t) -> Bool {
    kill(pid, 0) == 0
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
        Thread.sleep(forTimeInterval: interval)
    } while true
    return nil
}

func matches(_ pid: pid_t, _ selector: Selector) -> [Node] {
    guard let window = mainWindow(pid) else { return [] }
    return walk(window).filter { selector.matches(role: $0.role, names: $0.names) }
}

func waitFor(_ pid: pid_t, _ selector: Selector) -> Node {
    if let node = poll(selector.timeout, { () -> Node? in
        let found = matches(pid, selector)
        return found.count >= selector.nth ? found[selector.nth - 1] : nil
    }) {
        return node
    }
    if !processAlive(pid) { die("the development copy (process \(pid)) exited") }
    die("not found within \(selector.timeout) s: \(selector.describe)")
}

// MARK: - Window state

func setBool(_ element: AXUIElement, _ name: String, _ on: Bool) -> AXError {
    AXUIElementSetAttributeValue(element, name as CFString, (on ? kCFBooleanTrue : kCFBooleanFalse) as CFTypeRef)
}

func bool(_ element: AXUIElement, _ name: String) -> Bool {
    (attribute(element, name) as? Bool) ?? false
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
        die("could not bring process \(pid) forward; refusing to send input", 3)
    }
}

/// The visible part of the main screen in accessibility coordinates (top left).
func visibleFrame() -> CGRect {
    guard let screen = NSScreen.main, let primary = NSScreen.screens.first else { return .zero }
    let visible = screen.visibleFrame
    return CGRect(x: visible.minX, y: primary.frame.height - visible.maxY, width: visible.width, height: visible.height)
}

func windowNumber(_ pid: pid_t) -> Int? {
    let list = (CGWindowListCopyWindowInfo([.optionAll], kCGNullWindowID) as? [[String: Any]]) ?? []
    let mine = list.filter {
        ($0[kCGWindowOwnerPID as String] as? Int32) == pid && ($0[kCGWindowLayer as String] as? Int) == 0
    }
    let largest = mine.max { area($0) < area($1) }
    return largest?[kCGWindowNumber as String] as? Int
}

func area(_ info: [String: Any]) -> CGFloat {
    guard let bounds = info[kCGWindowBounds as String] as! CFDictionary?,
          let rect = CGRect(dictionaryRepresentation: bounds) else { return 0 }
    return rect.width * rect.height
}

/// The owner of the frontmost on-screen window under `point`.
func ownerOfWindow(at point: CGPoint) -> pid_t? {
    let list = (CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]]) ?? []
    for info in list {
        guard let bounds = info[kCGWindowBounds as String] as! CFDictionary?,
              let rect = CGRect(dictionaryRepresentation: bounds), rect.contains(point) else { continue }
        if let alpha = info[kCGWindowAlpha as String] as? Double, alpha == 0 { continue }
        return info[kCGWindowOwnerPID as String] as? pid_t
    }
    return nil
}

// MARK: - Input

func postKey(_ pid: pid_t, _ combo: Combo) {
    let source = CGEventSource(stateID: .hidSystemState)
    for down in [true, false] {
        guard let event = CGEvent(keyboardEventSource: source, virtualKey: combo.key, keyDown: down) else {
            die("could not make a key event")
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
        guard ownerOfWindow(at: point) == pid else {
            die("refused: the window under \(fmt(point.x)), \(fmt(point.y)) is not the development copy's", 3)
        }
    }
    let flipped = NSEvent.mouseLocation
    let primary = NSScreen.screens.first?.frame.height ?? 0
    let original = CGPoint(x: flipped.x, y: primary - flipped.y)
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
    check(widestGap(occupied: [(0, 10), (40, 50)], from: 0, to: 100, minimum: 10) == 75, "widest gap at the end")
    check(widestGap(occupied: [(0, 10), (60, 100)], from: 0, to: 100, minimum: 10) == 35, "widest gap between")
    check(widestGap(occupied: [(0, 95)], from: 0, to: 100, minimum: 10) == nil, "too narrow")
    check(widestGap(occupied: [(5, 20), (10, 30)], from: 0, to: 40, minimum: 4) == 35, "overlaps merge")
    if failures > 0 { exit(1) }
    print("axdrive self-test passed")
}

// MARK: - Commands

let usage = """
usage: axdrive self-test | preflight
       axdrive COMMAND PID [selector] [arguments]
commands with a PID:
  wait-window, window, window-id, place W H, lights, front, dump,
  find, frame, absent, count, press, focus, focused, selected, value, set-value --text T,
  growth, key COMBO, type --text T [--delay S], fullscreen on|off,
  minimized on|off|get, hit X Y, bar-gap FROM TO,
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
            die(processAlive(pid) ? "no window within \(selector.timeout) s" : "process \(pid) exited")
        }
        say(fmt(rect))
    case "window":
        let window = requireWindow(pid)
        guard let rect = frame(of: window) else { die("window has no frame") }
        say("\(fmt(rect)) \(bool(window, "AXFullScreen") ? 1 : 0) \(bool(window, kAXMinimizedAttribute) ? 1 : 0)")
    case "window-id":
        guard let number = windowNumber(pid) else { die("no on-screen window for process \(pid)") }
        say(String(number))
    case "place":
        guard rest.count == 2 else { die(usage, 2) }
        let window = requireWindow(pid)
        let visible = visibleFrame()
        let width = min(number(rest[0]), visible.width - 40)
        let height = min(number(rest[1]), visible.height - 40)
        var origin = CGPoint(x: visible.minX + 20, y: visible.minY + 20)
        var size = CGSize(width: width, height: height)
        _ = AXUIElementSetAttributeValue(window, kAXPositionAttribute as CFString, AXValueCreate(.cgPoint, &origin)!)
        _ = AXUIElementSetAttributeValue(window, kAXSizeAttribute as CFString, AXValueCreate(.cgSize, &size)!)
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
        let gone = poll(selector.timeout) { matches(pid, selector).isEmpty ? true : nil }
        if gone == nil { die("still present after \(selector.timeout) s: \(selector.describe)") }
    case "press":
        let node = waitFor(pid, selector)
        let result = AXUIElementPerformAction(node.element, kAXPressAction as CFString)
        guard result == .success else { die("press failed (\(result.rawValue)) on \(selector.describe)") }
        say("pressed \(node.role) \(node.name.prefix(120))")
    case "focus":
        let node = waitFor(pid, selector)
        let result = setBool(node.element, kAXFocusedAttribute, true)
        guard result == .success else { die("focus failed (\(result.rawValue)) on \(selector.describe)") }
    case "focused":
        say(bool(waitFor(pid, selector).element, kAXFocusedAttribute) ? "1" : "0")
    case "selected":
        let element = waitFor(pid, selector).element
        // accesskit_macos 0.26.3 exposes tabs' selection as AXValue.
        let selected = bool(element, kAXSelectedAttribute) || bool(element, kAXValueAttribute)
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
        var lengths = Set<Int>()
        var seen = false
        let deadline = Date().addingTimeInterval(selector.timeout)
        while Date() < deadline {
            let found = matches(pid, selector)
            if let node = found.first {
                seen = true
                lengths.insert(node.name.count)
            } else if seen {
                break
            }
            Thread.sleep(forTimeInterval: 0.05)
        }
        guard seen else { die("never saw \(selector.describe)") }
        say("\(lengths.count) different lengths, longest \(lengths.max() ?? 0) characters")
        if lengths.count < 2 { exit(1) }
    case "key":
        guard let text = rest.first else { die(usage, 2) }
        let combo = try parseCombo(text)
        front(pid)
        postKey(pid, combo)
    case "type":
        guard let text = extra["--text"] else { die(usage, 2) }
        front(pid)
        postText(pid, text, delay: Double(extra["--delay"] ?? "0.008") ?? 0.008)
    case "fullscreen":
        guard let state = rest.first, state == "on" || state == "off" else { die(usage, 2) }
        let window = requireWindow(pid)
        let wanted = state == "on"
        let result = setBool(window, "AXFullScreen", wanted)
        guard result == .success else { die("AXFullScreen could not be set (\(result.rawValue))") }
        guard poll(selector.timeout, interval: 0.2, { bool(window, "AXFullScreen") == wanted ? true : nil }) != nil else {
            die("full screen still \(wanted ? "off" : "on") after \(selector.timeout) s")
        }
        // The space animation runs after the attribute changes.
        Thread.sleep(forTimeInterval: 1.5)
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
        var element: AXUIElement?
        let result = AXUIElementCopyElementAtPosition(AXUIElementCreateSystemWide(), Float(number(rest[0])), Float(number(rest[1])), &element)
        guard result == .success, let element else { die("nothing at that point") }
        var owner: pid_t = 0
        AXUIElementGetPid(element, &owner)
        let (node, _) = read(element, depth: 0)
        say("owner \(owner) \(node.role) \(node.name.prefix(80))")
        if owner != pid || expandRoles("control,field").contains(node.role) { exit(1) }
    case "bar-gap":
        // A point on the 44-point bar that no element covers.
        guard rest.count == 2 else { die(usage, 2) }
        let window = requireWindow(pid)
        guard let rect = frame(of: window) else { die("window has no frame") }
        let band = CGRect(x: rect.minX, y: rect.minY, width: rect.width, height: 44)
        var occupied = walk(window).compactMap { node -> (CGFloat, CGFloat)? in
            guard let box = node.frame, box.height <= 46, box.intersects(band),
                  expandRoles("control,field,text,AXSplitter").contains(node.role) else { return nil }
            return (box.minX - 6, box.maxX + 6)
        }
        for name in [kAXCloseButtonAttribute, kAXMinimizeButtonAttribute, kAXZoomButtonAttribute] {
            if let button = attribute(window, name), CFGetTypeID(button) == AXUIElementGetTypeID(),
               let box = frame(of: button as! AXUIElement) { occupied.append((box.minX - 6, box.maxX + 6)) }
        }
        guard let x = widestGap(occupied: occupied, from: number(rest[0]), to: number(rest[1]), minimum: 24) else {
            die("no free stretch of the bar between \(rest[0]) and \(rest[1])")
        }
        say("\(fmt(x)) \(fmt(rect.minY + 22))")
    case "drag":
        guard rest.count == 4 else { die(usage, 2) }
        let from = CGPoint(x: number(rest[0]), y: number(rest[1]))
        let to = CGPoint(x: number(rest[2]), y: number(rest[3]))
        front(pid)
        pointer(pid, at: [from]) { drag(from, to) }
    case "select-text":
        guard let expected = extra["--text"], !expected.isEmpty else { die(usage, 2) }
        let node = waitFor(pid, selector)
        guard let box = node.frame, box.width > 80, box.height >= 12 else { die("message has no selectable frame") }
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
        let from = CGPoint(x: box.minX + 3, y: box.minY + 10)
        let to = CGPoint(x: min(box.maxX - 3, box.minX + 160), y: from.y)
        pointer(pid, at: [from, to]) { drag(from, to) }
        board.clearContents()
        postKey(pid, try parseCombo("cmd+c"))
        guard let copied = poll(2, { board.string(forType: .string) }),
              copied.count >= 3, expected.contains(copied), copied != expected else {
            die("drag and Cmd-C did not copy a proper substring of the fixture message")
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

do {
    try run(Array(CommandLine.arguments.dropFirst()))
} catch {
    die("\(error)\n\(usage)", 2)
}
