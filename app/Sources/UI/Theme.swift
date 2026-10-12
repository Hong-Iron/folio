import AppKit
import SwiftUI

// Paper and ink. Every color has a light and a dark value; nothing else in
// the app picks colors directly.
enum Palette {
    static func dynamic(_ light: UInt32, _ dark: UInt32) -> NSColor {
        NSColor(name: nil) { appearance in
            let dark0 = appearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
            return NSColor(hex: dark0 ? dark : light)
        }
    }

    static let paperNS = dynamic(0xF4EEE3, 0x1E1B17)
    static let raisedNS = dynamic(0xFBF8F2, 0x27231E)
    static let sunkNS = dynamic(0xEAE2D3, 0x2E2924)
    static let ruleNS = dynamic(0xDCD2C0, 0x3A342D)
    static let inkNS = dynamic(0x2B251E, 0xEDE6DA)
    static let ink2NS = dynamic(0x675D50, 0xB9AF9F)
    static let ink3NS = dynamic(0x8E8374, 0x8A8070)
    static let mossNS = dynamic(0x5E7D4E, 0x8DAA78)
    static let sageNS = dynamic(0xA9B78F, 0x5F7050)
    static let clayNS = dynamic(0xC2633A, 0xD9805A)
    static let stoneNS = dynamic(0xD3C9B8, 0x4A443C)

    static let paper = Color(nsColor: paperNS)
    static let raised = Color(nsColor: raisedNS)
    static let sunk = Color(nsColor: sunkNS)
    static let rule = Color(nsColor: ruleNS)
    static let ink = Color(nsColor: inkNS)
    static let ink2 = Color(nsColor: ink2NS)
    static let ink3 = Color(nsColor: ink3NS)
    static let moss = Color(nsColor: mossNS)
    static let sage = Color(nsColor: sageNS)
    static let clay = Color(nsColor: clayNS)
    static let stone = Color(nsColor: stoneNS)
}

extension NSColor {
    convenience init(hex: UInt32, alpha: CGFloat = 1) {
        self.init(
            srgbRed: CGFloat((hex >> 16) & 0xFF) / 255,
            green: CGFloat((hex >> 8) & 0xFF) / 255,
            blue: CGFloat(hex & 0xFF) / 255,
            alpha: alpha
        )
    }
}

enum Fonts {
    /// New York, with tabular figures, for numbers and headlines.
    static func serif(_ size: CGFloat, _ weight: Font.Weight = .regular) -> Font {
        .system(size: size, weight: weight, design: .serif).monospacedDigit()
    }

    static func ui(_ size: CGFloat, _ weight: Font.Weight = .regular) -> Font {
        .system(size: size, weight: weight)
    }
}

extension Activity {
    var isFocus: Bool { self == .screenFocus || self == .deskFocus }
    var isDrift: Bool { self == .social || self == .phone || self == .dozing || self == .zoned }

    var color: Color {
        switch self {
        case .screenFocus: return Palette.moss
        case .deskFocus: return Palette.sage
        case .social, .phone, .dozing, .zoned: return Palette.clay
        case .away, .break: return Palette.stone
        case .unclear, .unmonitored: return .clear
        }
    }

    var nsColor: NSColor {
        switch self {
        case .screenFocus: return Palette.mossNS
        case .deskFocus: return Palette.sageNS
        case .social, .phone, .dozing, .zoned: return Palette.clayNS
        case .away, .break: return Palette.stoneNS
        case .unclear, .unmonitored: return Palette.ink3NS
        }
    }

    var dashed: Bool { self == .unclear || self == .unmonitored }

    func phrase(detail: String?) -> String {
        switch self {
        case .screenFocus: return "At the screen"
        case .deskFocus: return "At your desk"
        case .social: return "On \(detail ?? "a distracting site")"
        case .phone: return "On your phone"
        case .dozing: return "Resting your eyes"
        case .zoned: return "Looking away"
        case .unclear: return "Not sure what you're doing"
        case .away: return "Away"
        case .unmonitored: return "Can't see you clearly"
        case .break: return "On a break"
        }
    }

    var legend: String {
        switch self {
        case .screenFocus: return "At the screen"
        case .deskFocus: return "At the desk"
        case .social, .phone, .dozing, .zoned: return "Drifted"
        case .away, .break: return "Away or break"
        case .unclear: return "Unclear"
        case .unmonitored: return "Not monitored"
        }
    }
}

// MARK: - Components

struct InkButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(Fonts.ui(13))
            .lineLimit(1)
            .fixedSize()
            .foregroundStyle(Palette.paper)
            .padding(.horizontal, 12)
            .padding(.vertical, 6)
            .background(RoundedRectangle(cornerRadius: 7).fill(Palette.ink))
            .opacity(configuration.isPressed ? 0.8 : 1)
            .contentShape(Rectangle())
    }
}

struct QuietButtonStyle: ButtonStyle {
    var small = false
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(Fonts.ui(small ? 12 : 13))
            .lineLimit(1)
            .fixedSize()
            .foregroundStyle(Palette.ink)
            .padding(.horizontal, small ? 10 : 12)
            .padding(.vertical, small ? 5 : 6)
            .background(
                RoundedRectangle(cornerRadius: 7)
                    .fill(configuration.isPressed ? Palette.sunk : Palette.raised)
            )
            .overlay(RoundedRectangle(cornerRadius: 7).stroke(Palette.rule, lineWidth: 0.5))
            .contentShape(Rectangle())
    }
}

struct LinkButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(Fonts.ui(12))
            .foregroundStyle(configuration.isPressed ? Palette.ink : Palette.ink2)
            .contentShape(Rectangle())
    }
}

struct Chip: View {
    let title: String
    let selected: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Text(title)
                .font(Fonts.ui(12))
                .foregroundStyle(selected ? Palette.ink : Palette.ink2)
                .padding(.horizontal, 10)
                .padding(.vertical, 3)
                .background(Capsule().fill(selected ? Palette.sunk : Color.clear))
                .overlay(Capsule().strokeBorder(selected ? Palette.ink3 : Palette.rule, lineWidth: 0.5))
        }
        .buttonStyle(.plain)
    }
}

struct Hairline: View {
    var body: some View {
        Rectangle().fill(Palette.rule).frame(height: 0.5)
    }
}

struct Dot: View {
    let activity: Activity?
    var size: CGFloat = 8

    var body: some View {
        if let a = activity, !a.dashed {
            Circle().fill(a.color).frame(width: size, height: size)
        } else {
            Circle().stroke(Palette.ink3, style: StrokeStyle(lineWidth: 1, dash: [2, 1.5])).frame(width: size, height: size)
        }
    }
}

struct Swatch: View {
    let activity: Activity
    var body: some View {
        Group {
            if activity.dashed {
                RoundedRectangle(cornerRadius: 2).stroke(Palette.ink3, style: StrokeStyle(lineWidth: 1, dash: [2, 1.5]))
            } else {
                RoundedRectangle(cornerRadius: 2).fill(activity.color)
            }
        }
        .frame(width: 10, height: 10)
    }
}

struct Legend: View {
    var kinds: [Activity] = [.screenFocus, .deskFocus, .social, .away, .unclear]
    var body: some View {
        HStack(spacing: 14) {
            ForEach(kinds, id: \.self) { a in
                HStack(spacing: 5) {
                    Swatch(activity: a)
                    Text(a.legend).font(Fonts.ui(12)).foregroundStyle(Palette.ink2)
                }
            }
        }
    }
}

/// A session as a horizontal tape: one block per segment, 2pt gaps.
struct Ribbon: View {
    let segments: [SegmentView]
    var height: CGFloat = 12

    var body: some View {
        GeometryReader { geo in
            let start = segments.first?.startWall ?? 0
            let end = max(segments.last?.endWall ?? 1, start + 1)
            let span = Double(end - start)
            let gap: CGFloat = 2
            let usable = max(geo.size.width - gap * CGFloat(max(segments.count - 1, 0)), 1)
            HStack(spacing: gap) {
                ForEach(segments, id: \.id) { s in
                    let w = max(usable * CGFloat(Double(s.endWall - s.startWall) / span), 1)
                    block(s).frame(width: w)
                }
            }
        }
        .frame(height: height)
    }

    @ViewBuilder
    private func block(_ s: SegmentView) -> some View {
        let a = effective(s)
        if a.dashed {
            RoundedRectangle(cornerRadius: 2).strokeBorder(Palette.ink3, style: StrokeStyle(lineWidth: 1, dash: [3, 2]))
        } else {
            RoundedRectangle(cornerRadius: 2).fill(a.color)
        }
    }
}

func effective(_ s: SegmentView) -> Activity {
    if s.activity == .unclear && s.claimed {
        return s.detail == "desk" ? .deskFocus : .screenFocus
    }
    return s.activity
}

extension View {
    func card() -> some View {
        self.padding(16)
            .background(RoundedRectangle(cornerRadius: 12).fill(Palette.raised))
            .overlay(RoundedRectangle(cornerRadius: 12).stroke(Palette.rule, lineWidth: 0.5))
    }
}
