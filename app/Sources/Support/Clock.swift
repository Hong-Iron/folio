import AppKit
import Foundation

enum Clock {
    /// Monotonic milliseconds that keep counting through sleep, so a session
    /// that spans a closed lid measures real elapsed time.
    static func nowMs() -> UInt64 {
        clock_gettime_nsec_np(CLOCK_MONOTONIC) / 1_000_000
    }

    static func wallMs() -> Int64 {
        Int64(Date().timeIntervalSince1970 * 1000)
    }

    static func tzOffsetMin() -> Int32 {
        Int32(TimeZone.current.secondsFromGMT() / 60)
    }

    static func dateString(_ date: Date = Date()) -> String {
        let f = DateFormatter()
        f.calendar = Calendar(identifier: .gregorian)
        f.locale = Locale(identifier: "en_US_POSIX")
        f.dateFormat = "yyyy-MM-dd"
        return f.string(from: date)
    }

    static func date(from s: String) -> Date? {
        let f = DateFormatter()
        f.calendar = Calendar(identifier: .gregorian)
        f.locale = Locale(identifier: "en_US_POSIX")
        f.dateFormat = "yyyy-MM-dd"
        return f.date(from: s)
    }
}

enum Fmt {
    /// "1:42" (hours:minutes) for the menu bar and the session clock.
    static func clock(_ ms: UInt64) -> String {
        let m = ms / 60_000
        return "\(m / 60):" + String(format: "%02d", m % 60)
    }

    static func duration(_ ms: Int64) -> String {
        formatDurationMs(ms: ms)
    }

    static func duration(_ ms: UInt64) -> String {
        formatDurationMs(ms: Int64(ms))
    }

    static func time(_ wallMs: Int64) -> String {
        let f = DateFormatter()
        f.dateFormat = "HH:mm"
        return f.string(from: Date(timeIntervalSince1970: Double(wallMs) / 1000))
    }

    static func longDate(_ date: Date = Date()) -> String {
        let f = DateFormatter()
        f.dateFormat = "EEEE, MMMM d"
        return f.string(from: date)
    }

    static func partOfDay(_ wallMs: Int64) -> String {
        let h = Calendar.current.component(.hour, from: Date(timeIntervalSince1970: Double(wallMs) / 1000))
        let day: String = {
            let f = DateFormatter()
            f.dateFormat = "EEEE"
            return f.string(from: Date(timeIntervalSince1970: Double(wallMs) / 1000))
        }()
        switch h {
        case 5..<12: return "\(day) morning"
        case 12..<17: return "\(day) afternoon"
        case 17..<21: return "\(day) evening"
        default: return "\(day) night"
        }
    }
}

/// The app's mark (an open book with a dot above it), drawn in code so the
/// menu bar version is a crisp template image at any scale.
enum Glyph {
    static func draw(in rect: NSRect, book: NSColor, dot: NSColor) {
        let s = rect.width / 1024
        func p(_ x: CGFloat, _ y: CGFloat) -> NSPoint { NSPoint(x: rect.minX + x * s, y: rect.maxY - y * s) }

        dot.setFill()
        let r: CGFloat = 86 * s
        NSBezierPath(ovalIn: NSRect(x: p(512, 322).x - r, y: p(512, 322).y - r, width: r * 2, height: r * 2)).fill()

        book.setFill()
        for side in [-1.0, 1.0] as [CGFloat] {
            let inner: CGFloat = 512 + side * 8
            let outer: CGFloat = 512 + side * 266
            let path = NSBezierPath()
            path.move(to: p(inner, 472))
            path.curve(to: p(outer, 440), controlPoint1: p(512 + side * 82, 434), controlPoint2: p(512 + side * 186, 424))
            path.line(to: p(outer, 732))
            path.curve(to: p(inner, 766), controlPoint1: p(512 + side * 186, 718), controlPoint2: p(512 + side * 82, 728))
            path.close()
            path.fill()
        }
    }

    static func menuBarImage() -> NSImage {
        let img = NSImage(size: NSSize(width: 18, height: 18), flipped: false) { rect in
            draw(in: rect.insetBy(dx: -2, dy: -2), book: .black, dot: .black)
            return true
        }
        img.isTemplate = true
        return img
    }

    static func stateDot(_ activity: Activity?, paused: Bool) -> NSImage {
        NSImage(size: NSSize(width: 8, height: 8), flipped: false) { rect in
            let oval = NSBezierPath(ovalIn: rect.insetBy(dx: 0.75, dy: 0.75))
            if let a = activity, !a.dashed, !paused {
                a.nsColor.setFill()
                oval.fill()
            } else {
                Palette.ink3NS.setStroke()
                oval.lineWidth = 1.2
                oval.stroke()
            }
            return true
        }
    }
}
