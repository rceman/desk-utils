import AppKit
import Foundation
import IOKit

// MARK: - Private IOKit HID declarations (CPU temperature on Apple Silicon)

typealias IOHIDRef = UnsafeMutableRawPointer

@_silgen_name("IOHIDEventSystemClientCreate")
private func IOHIDEventSystemClientCreate(_ allocator: CFAllocator?) -> Unmanaged<AnyObject>?
@_silgen_name("IOHIDEventSystemClientSetMatching")
private func IOHIDEventSystemClientSetMatching(_ client: IOHIDRef, _ matching: CFDictionary)
@_silgen_name("IOHIDEventSystemClientCopyServices")
private func IOHIDEventSystemClientCopyServices(_ client: IOHIDRef) -> Unmanaged<AnyObject>?
@_silgen_name("IOHIDServiceClientCopyEvent")
private func IOHIDServiceClientCopyEvent(_ service: IOHIDRef, _ type: Int64, _ a: Int32, _ b: Int32) -> Unmanaged<AnyObject>?
@_silgen_name("IOHIDEventGetFloatValue")
private func IOHIDEventGetFloatValue(_ event: IOHIDRef, _ field: UInt32) -> Double
@_silgen_name("IOHIDServiceClientCopyProperty")
private func IOHIDServiceClientCopyProperty(_ service: IOHIDRef, _ key: CFString) -> Unmanaged<AnyObject>?

private let kIOHIDEventTypeTemperature: Int64 = 15

// MARK: - Metrics

struct Metrics {
    var cpuUser: Double = 0
    var cpuSystem: Double = 0
    var cpuTotal: Double = 0
    var ramUsed: UInt64 = 0
    var ramTotal: UInt64 = 0
    var ramPercent: Double = 0
    var diskUsed: UInt64 = 0
    var diskFree: UInt64 = 0
    var diskPercent: Double = 0
    var temp: Double? = nil
    var dvn: Double? = nil
    var cdx: Double? = nil
}

final class MetricsSampler {
    private var prevTicks: (user: UInt64, system: UInt64, idle: UInt64, nice: UInt64)?
    private var pageSize: UInt64 = 16384
    private var hidClient: (obj: AnyObject, ref: IOHIDRef)?

    init() {
        var ps = vm_size_t(0)
        host_page_size(mach_host_self(), &ps)
        pageSize = UInt64(ps)
        _ = sampleCPU()
    }

    func sample() -> Metrics {
        var m = Metrics()
        let cpu = sampleCPU()
        m.cpuUser = cpu.user; m.cpuSystem = cpu.system; m.cpuTotal = cpu.total
        sampleRAM(&m)
        sampleDisk(&m)
        m.temp = sampleTemp()
        return m
    }

    private func sampleCPU() -> (user: Double, system: Double, total: Double) {
        var count = mach_msg_type_number_t(MemoryLayout<host_cpu_load_info>.size / MemoryLayout<integer_t>.size)
        var info = host_cpu_load_info()
        let kr = withUnsafeMutablePointer(to: &info) {
            $0.withMemoryRebound(to: integer_t.self, capacity: Int(count)) {
                host_statistics(mach_host_self(), HOST_CPU_LOAD_INFO, $0, &count)
            }
        }
        guard kr == KERN_SUCCESS else { return (0, 0, 0) }
        let cur = (user: UInt64(info.cpu_ticks.0), system: UInt64(info.cpu_ticks.1),
                   idle: UInt64(info.cpu_ticks.2), nice: UInt64(info.cpu_ticks.3))
        defer { prevTicks = cur }
        guard let p = prevTicks else { return (0, 0, 0) }
        let dUser = Double(cur.user - p.user), dSys = Double(cur.system - p.system),
            dIdle = Double(cur.idle - p.idle), dNice = Double(cur.nice - p.nice)
        let total = dUser + dSys + dIdle + dNice
        guard total > 0 else { return (0, 0, 0) }
        let user = (dUser + dNice) / total * 100
        let sys = dSys / total * 100
        return (user, sys, user + sys)
    }

    private func sampleRAM(_ m: inout Metrics) {
        var count = mach_msg_type_number_t(MemoryLayout<vm_statistics64>.size / MemoryLayout<integer_t>.size)
        var info = vm_statistics64()
        let kr = withUnsafeMutablePointer(to: &info) {
            $0.withMemoryRebound(to: integer_t.self, capacity: Int(count)) {
                host_statistics64(mach_host_self(), HOST_VM_INFO64, $0, &count)
            }
        }
        guard kr == KERN_SUCCESS else { return }
        let used = (UInt64(info.active_count) + UInt64(info.wire_count) + UInt64(info.compressor_page_count)) * pageSize
        let total = ProcessInfo.processInfo.physicalMemory
        m.ramUsed = used
        m.ramTotal = total
        m.ramPercent = total > 0 ? Double(used) / Double(total) * 100 : 0
    }

    private func sampleDisk(_ m: inout Metrics) {
        guard let vals = try? URL(fileURLWithPath: "/").resourceValues(
            forKeys: [.volumeTotalCapacityKey, .volumeAvailableCapacityForImportantUsageKey]),
            let total = vals.volumeTotalCapacity,
            let avail = vals.volumeAvailableCapacityForImportantUsage else { return }
        let total64 = Int64(total), avail64 = Int64(avail)
        m.diskFree = UInt64(max(0, avail64))
        m.diskUsed = UInt64(max(0, total64 - avail64))
        m.diskPercent = total64 > 0 ? Double(total64 - avail64) / Double(total64) * 100 : 0
    }

    private func sampleTemp() -> Double? {
        if hidClient == nil {
            guard let obj = IOHIDEventSystemClientCreate(nil) else { return nil }
            let retained = obj.takeRetainedValue()
            hidClient = (retained, Unmanaged.passUnretained(retained).toOpaque())
            IOHIDEventSystemClientSetMatching(hidClient!.ref,
                ["PrimaryUsagePage": 0xff00, "PrimaryUsage": 5] as CFDictionary)
        }
        guard let servicesObj = IOHIDEventSystemClientCopyServices(hidClient!.ref) else { return nil }
        let services = servicesObj.takeRetainedValue() as! [AnyObject]
        var tdie: [Double] = []
        var all: [Double] = []
        for s in services {
            let svc = Unmanaged.passUnretained(s).toOpaque()
            guard let e = IOHIDServiceClientCopyEvent(svc, kIOHIDEventTypeTemperature, 0, 0) else { continue }
            let ev = e.takeRetainedValue()
            let t = IOHIDEventGetFloatValue(Unmanaged.passUnretained(ev).toOpaque(), UInt32(kIOHIDEventTypeTemperature) << 16)
            guard t > -50, t < 150 else { continue }
            all.append(t)
            if let p = IOHIDServiceClientCopyProperty(svc, "Product" as CFString),
               let name = p.takeRetainedValue() as? String,
               name.lowercased().contains("tdie") {
                tdie.append(t)
            }
        }
        return tdie.max() ?? all.max()
    }
}

// MARK: - Menu bar rendering

enum Prefs {
    static let d = UserDefaults.standard
    static var spacing: CGFloat { d.object(forKey: "spacing") as? Double != nil ? CGFloat(d.double(forKey: "spacing")) : 2 }
    static var fontSize: CGFloat { d.object(forKey: "fontSize") as? Double != nil ? CGFloat(d.double(forKey: "fontSize")) : 9.5 }
    static var vOffset: CGFloat { d.object(forKey: "vOffset") as? Double != nil ? CGFloat(d.double(forKey: "vOffset")) : 0 }
    static var hidden: Set<String> { Set(d.stringArray(forKey: "hidden") ?? []) }
}

private var labelFont: NSFont { NSFont.monospacedSystemFont(ofSize: Prefs.fontSize, weight: .semibold) }
private var valueFont: NSFont { NSFont.monospacedSystemFont(ofSize: Prefs.fontSize, weight: .semibold) }
private var columnSpacing: CGFloat { Prefs.spacing }
private let dividerSpacing: CGFloat = 4
private let edgePadding: CGFloat = 0

private func valueColor(percent: Double) -> NSColor {
    if percent >= 90 { return .systemRed }
    if percent >= 70 { return .systemOrange }
    return .labelColor
}

private func tempColor(_ t: Double) -> NSColor {
    if t >= 90 { return .systemRed }
    if t >= 75 { return .systemOrange }
    return .labelColor
}

func pctText(_ v: Double) -> String {
    let i = Int(v.rounded())
    return i >= 100 ? "100" : "\(i)%"
}

func renderImage(m: Metrics) -> NSImage {
    struct Col { let label: String; let value: String; let color: NSColor }
    let usage: [Col] = [
        Col(label: "CPU", value: pctText(m.cpuTotal), color: valueColor(percent: m.cpuTotal)),
        Col(label: "RAM", value: pctText(m.ramPercent), color: valueColor(percent: m.ramPercent)),
        Col(label: "DSK", value: pctText(m.diskPercent), color: valueColor(percent: m.diskPercent)),
    ]
    var tempCol: Col? = nil
    if let t = m.temp {
        tempCol = Col(label: "TMP", value: String(format: "%d\u{00B0}", Int(t.rounded())), color: tempColor(t))
    }
    var cdxCol: Col? = nil
    if let c = m.cdx {
        cdxCol = Col(label: "CDX", value: pctText(c), color: c <= 10 ? .systemRed : c <= 25 ? .systemOrange : .labelColor)
    }
    var dvnCol: Col? = nil
    if let d = m.dvn {
        dvnCol = Col(label: "DVN", value: pctText(d), color: d <= 10 ? .systemRed : d <= 25 ? .systemOrange : .labelColor)
    }

    let h = NSStatusBar.system.thickness
    let labelAttrs: [NSAttributedString.Key: Any] = [.font: labelFont, .foregroundColor: NSColor.secondaryLabelColor]

    func colWidth(_ c: Col) -> CGFloat {
        let lw = (c.label as NSString).size(withAttributes: labelAttrs).width
        let vw = (c.value as NSString).size(withAttributes: [.font: valueFont]).width
        return ceil(max(lw, vw))
    }

    let cols = (usage + [tempCol, dvnCol, cdxCol].compactMap { $0 }).filter { !Prefs.hidden.contains($0.label) }
    let widths = cols.map(colWidth)

    var totalW = edgePadding * 2
    for (i, w) in widths.enumerated() {
        totalW += w
        if i < cols.count - 1 {
            totalW += (false) ? dividerSpacing * 2 + 1 : columnSpacing
        }
    }

    let gap = Prefs.fontSize * (2.5 / 9.5)
    let margin = (h - labelFont.capHeight - valueFont.capHeight - gap) / 2 + Prefs.vOffset
    let valBase = margin
    let lblBase = margin + valueFont.capHeight + gap
    let img = NSImage(size: NSSize(width: totalW, height: h), flipped: false) { rect in
        NSAppearance.current = NSApp.effectiveAppearance
        var x = edgePadding
        for (i, c) in cols.enumerated() {
            let w = widths[i]
            // label centered on top
            let lw = (c.label as NSString).size(withAttributes: labelAttrs).width
            var la = labelAttrs
            if c.label == "DVN" || c.label == "CDX" { la[.foregroundColor] = NSColor(white: 0.78, alpha: 1) }
            (c.label as NSString).draw(at: NSPoint(x: x + (w - lw) / 2, y: lblBase + labelFont.descender), withAttributes: la)
            // value below
            let vAttrs: [NSAttributedString.Key: Any] = [.font: valueFont, .foregroundColor: c.color]
            let vw = (c.value as NSString).size(withAttributes: vAttrs).width
            (c.value as NSString).draw(at: NSPoint(x: x + (w - vw) / 2, y: valBase + valueFont.descender), withAttributes: vAttrs)
            x += w
            if i < cols.count - 1 {
                if false {
                    let dx = x + dividerSpacing
                    NSColor.separatorColor.set()
                    NSRect(x: dx, y: 3, width: 1, height: h - 6).fill()
                    x += dividerSpacing * 2 + 1
                } else {
                    x += columnSpacing
                }
            }
        }
        return true
    }
    img.isTemplate = false
    return img
}

// MARK: - Devin weekly quota

func fetchDevinWeeklyRemaining(_ done: @escaping (Double?, Date?) -> Void) {
    let path = NSHomeDirectory() + "/.local/share/devin/credentials.toml"
    guard let text = try? String(contentsOfFile: path, encoding: .utf8) else { return done(nil, nil) }
    func val(_ k: String) -> String? {
        for line in text.split(separator: "\n") {
            let parts = line.split(separator: "=", maxSplits: 1).map { $0.trimmingCharacters(in: .whitespaces) }
            if parts.count == 2, parts[0] == k { return parts[1].trimmingCharacters(in: CharacterSet(charactersIn: "\"'")) }
        }
        return nil
    }
    guard let key = val("windsurf_api_key"), let base = val("api_server_url"),
          let url = URL(string: base.trimmingCharacters(in: CharacterSet(charactersIn: "/")) + "/exa.seat_management_pb.SeatManagementService/GetUserStatus")
    else { return done(nil, nil) }
    var req = URLRequest(url: url, timeoutInterval: 20)
    req.httpMethod = "POST"
    req.setValue("application/json", forHTTPHeaderField: "Content-Type")
    req.setValue("1", forHTTPHeaderField: "Connect-Protocol-Version")
    let meta: [String: Any] = ["apiKey": key, "ideName": "devin-cli", "ideVersion": "3000.11.228", "extensionName": "devin-cli", "extensionVersion": "3000.11.228"]
    req.httpBody = try? JSONSerialization.data(withJSONObject: ["metadata": meta])
    URLSession.shared.dataTask(with: req) { data, _, _ in
        guard let data = data,
              let j = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let us = j["userStatus"] as? [String: Any],
              let ps = us["planStatus"] as? [String: Any],
              let rem = ps["weeklyQuotaRemainingPercent"] as? Double
        else { return done(nil, nil) }
        let reset = (ps["weeklyQuotaResetAtUnix"] as? String).flatMap { Double($0) }.map { Date(timeIntervalSince1970: $0) }
        done(rem, reset)
    }.resume()
}

// MARK: - Codex 5-hour limit

func fetchCodexFiveHourRemaining(_ done: @escaping (Double?, Date?) -> Void) {
    guard let data = FileManager.default.contents(atPath: NSHomeDirectory() + "/.codex/auth.json"),
          let a = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
          let t = a["tokens"] as? [String: Any], let token = t["access_token"] as? String,
          let url = URL(string: "https://chatgpt.com/backend-api/wham/usage") else { return done(nil, nil) }
    var req = URLRequest(url: url, timeoutInterval: 20)
    req.setValue("Bearer " + token, forHTTPHeaderField: "Authorization")
    req.setValue("codex-cli", forHTTPHeaderField: "User-Agent")
    if let acc = t["account_id"] as? String { req.setValue(acc, forHTTPHeaderField: "ChatGPT-Account-Id") }
    URLSession.shared.dataTask(with: req) { data, _, _ in
        guard let data = data,
              let j = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let rl = j["rate_limit"] as? [String: Any],
              let pw = rl["primary_window"] as? [String: Any],
              let used = pw["used_percent"] as? Double else { return done(nil, nil) }
        done(max(0, 100 - used), (pw["reset_at"] as? Double).map { Date(timeIntervalSince1970: $0) })
    }.resume()
}

// MARK: - App

final class AppDelegate: NSObject, NSApplicationDelegate, NSMenuDelegate {
    private let statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
    private let sampler = MetricsSampler()
    private var timer: Timer?
    private var last = Metrics()
    private var dvn: Double? = nil
    private var dvnReset: Date? = nil
    private var cdx: Double? = nil
    private var cdxReset: Date? = nil

    func applicationDidFinishLaunching(_ n: Notification) {
        let menu = NSMenu()
        menu.addItem(withTitle: "CPU: —", action: nil, keyEquivalent: "").isEnabled = false
        menu.addItem(withTitle: "RAM: —", action: nil, keyEquivalent: "").isEnabled = false
        menu.addItem(withTitle: "Disk: —", action: nil, keyEquivalent: "").isEnabled = false
        menu.addItem(withTitle: "Temp: —", action: nil, keyEquivalent: "").isEnabled = false
        menu.addItem(withTitle: "Devin weekly: —", action: nil, keyEquivalent: "").isEnabled = false
        menu.addItem(withTitle: "Codex 5h: —", action: nil, keyEquivalent: "").isEnabled = false
        menu.addItem(.separator())
        addSettings(to: menu)
        menu.addItem(.separator())
        menu.addItem(withTitle: "Quit", action: #selector(quit), keyEquivalent: "q")
        statusItem.menu = menu
        timer = Timer.scheduledTimer(withTimeInterval: 2, repeats: true) { [weak self] _ in self?.tick() }
        RunLoop.main.add(timer!, forMode: .common)
        tick()
        refreshDvn()
        refreshCdx()
        let dt = Timer.scheduledTimer(withTimeInterval: 300, repeats: true) { [weak self] _ in self?.refreshDvn() }
        RunLoop.main.add(dt, forMode: .common)
        let ct = Timer.scheduledTimer(withTimeInterval: 300, repeats: true) { [weak self] _ in self?.refreshCdx() }
        RunLoop.main.add(ct, forMode: .common)
    }

    private func refreshCdx() {
        fetchCodexFiveHourRemaining { [weak self] rem, reset in
            DispatchQueue.main.async {
                self?.cdx = rem; self?.cdxReset = reset
                self?.tick()
            }
        }
    }

    private func refreshDvn() {
        fetchDevinWeeklyRemaining { [weak self] rem, reset in
            DispatchQueue.main.async {
                if let rem = rem { self?.dvn = rem; self?.dvnReset = reset }
                self?.tick()
            }
        }
    }

    private func addSettings(to menu: NSMenu) {
        let spacing = NSMenuItem(title: "Column spacing", action: nil, keyEquivalent: "")
        let sm = NSMenu()
        for v in [0.0, 1, 2, 3, 4, 5, 6, 8] {
            let it = NSMenuItem(title: "\(v) pt", action: #selector(setSpacing(_:)), keyEquivalent: "")
            it.target = self; it.representedObject = v; sm.addItem(it)
        }
        spacing.submenu = sm; menu.addItem(spacing)
        let font = NSMenuItem(title: "Font size", action: nil, keyEquivalent: "")
        let fm = NSMenu()
        for v in [8.0, 8.5, 9, 9.5, 10, 10.5, 11] {
            let it = NSMenuItem(title: "\(v) pt", action: #selector(setFontSize(_:)), keyEquivalent: "")
            it.target = self; it.representedObject = v; fm.addItem(it)
        }
        font.submenu = fm; menu.addItem(font)
        let vo = NSMenuItem(title: "Vertical offset", action: nil, keyEquivalent: "")
        let vm = NSMenu()
        for v in [-1.0, -0.75, -0.5, -0.25, 0, 0.25, 0.5, 0.75, 1.0] {
            let it = NSMenuItem(title: (v > 0 ? "+" : "") + "\(v) pt" + (v > 0 ? " (up)" : v < 0 ? " (down)" : ""), action: #selector(setVOffset(_:)), keyEquivalent: "")
            it.target = self; it.representedObject = v; vm.addItem(it)
        }
        vo.submenu = vm; menu.addItem(vo)
        let show = NSMenuItem(title: "Show columns", action: nil, keyEquivalent: "")
        let shm = NSMenu()
        for l in ["CPU", "RAM", "DSK", "TMP", "DVN", "CDX"] {
            let it = NSMenuItem(title: l, action: #selector(toggleColumn(_:)), keyEquivalent: "")
            it.target = self; it.representedObject = l; shm.addItem(it)
        }
        show.submenu = shm; menu.addItem(show)
        menu.delegate = self
    }

    func menuNeedsUpdate(_ menu: NSMenu) {
        let vcur = Double(Prefs.vOffset), cur = Double(Prefs.spacing), fs = Double(Prefs.fontSize), hid = Prefs.hidden
        for top in menu.items {
            for it in top.submenu?.items ?? [] {
                if it.action == #selector(setSpacing(_:)) { it.state = (it.representedObject as? Double) == cur ? .on : .off }
                if it.action == #selector(setVOffset(_:)) { it.state = (it.representedObject as? Double) == vcur ? .on : .off }
                if it.action == #selector(setFontSize(_:)) { it.state = (it.representedObject as? Double) == fs ? .on : .off }
                if it.action == #selector(toggleColumn(_:)) { it.state = hid.contains(it.representedObject as? String ?? "") ? .off : .on }
            }
        }
    }

    @objc private func setSpacing(_ s: NSMenuItem) { Prefs.d.set(s.representedObject as? Double, forKey: "spacing"); tick() }
    @objc private func setVOffset(_ s: NSMenuItem) { Prefs.d.set(s.representedObject as? Double, forKey: "vOffset"); tick() }
    @objc private func setFontSize(_ s: NSMenuItem) { Prefs.d.set(s.representedObject as? Double, forKey: "fontSize"); tick() }
    @objc private func toggleColumn(_ s: NSMenuItem) {
        guard let l = s.representedObject as? String else { return }
        var h = Prefs.hidden
        if h.contains(l) { h.remove(l) } else { h.insert(l) }
        Prefs.d.set(Array(h), forKey: "hidden"); tick()
    }

    @objc private func quit() { NSApp.terminate(nil) }

    private func tick() {
        last = sampler.sample()
        last.dvn = dvn
        last.cdx = cdx
        statusItem.button?.image = renderImage(m: last)
        guard let items = statusItem.menu?.items else { return }
        let gb = { (b: UInt64) in String(format: "%.1f", Double(b) / 1_073_741_824) }
        items[0].title = String(format: "CPU: %d%%  (user %d%%, sys %d%%)",
            Int(last.cpuTotal.rounded()), Int(last.cpuUser.rounded()), Int(last.cpuSystem.rounded()))
        items[1].title = "RAM: \(gb(last.ramUsed)) / \(gb(last.ramTotal)) GB used, \(gb(last.ramTotal > last.ramUsed ? last.ramTotal - last.ramUsed : 0)) GB free"
        items[2].title = "Disk /: \(gb(last.diskUsed)) used, \(gb(last.diskFree)) GB free (\(Int(last.diskPercent.rounded()))%)"
        items[3].title = last.temp.map { String(format: "CPU Temp: %.1f \u{00B0}C", $0) } ?? "CPU Temp: n/a"
        if let d = dvn {
            let f = DateFormatter(); f.dateFormat = "d MMM, HH:mm"
            items[4].title = "Devin weekly: \(Int(d.rounded()))% left" + (dvnReset.map { ", resets \(f.string(from: $0))" } ?? "")
        } else { items[4].title = "Devin weekly: n/a" }
        if let c = cdx {
            let f = DateFormatter(); f.dateFormat = "HH:mm"
            items[5].title = "Codex 5h: \(Int(c.rounded()))% left" + (cdxReset.map { ", resets \(f.string(from: $0))" } ?? "")
        } else { items[5].title = "Codex 5h: n/a" }
    }
}

let app = NSApplication.shared
app.setActivationPolicy(.accessory)
let delegate = AppDelegate()
app.delegate = delegate
app.run()
