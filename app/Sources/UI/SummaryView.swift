import SwiftUI

struct SummaryView: View {
    @State var summary: SessionSummary

    var body: some View {
        ScrollView {
            SummaryContent(summary: $summary)
        }
        .background(Palette.paper)
    }
}

struct SummaryContent: View {
    @EnvironmentObject var model: AppModel
    @Binding var summary: SessionSummary
    @State private var dismissed: Set<Int64> = []

    var body: some View {
            VStack(alignment: .leading, spacing: 0) {
                Text(header).font(Fonts.ui(12)).foregroundStyle(Palette.ink3)
                Text(summary.headline).font(Fonts.serif(34)).foregroundStyle(Palette.ink)
                    .padding(.top, 6)
                Text(summary.sentence).font(Fonts.ui(14)).foregroundStyle(Palette.ink2)
                    .fixedSize(horizontal: false, vertical: true)
                    .frame(maxWidth: 480, alignment: .leading)
                    .padding(.top, 6)

                if !summary.segments.isEmpty {
                    Ribbon(segments: summary.segments, height: 18).padding(.top, 22)
                    TimeAxis(start: summary.startedWall, end: summary.endedWall).padding(.top, 4)
                    Legend().padding(.top, 8)
                }

                LazyVGrid(columns: Array(repeating: GridItem(.flexible(), alignment: .leading), count: 4), spacing: 12) {
                    Stat(label: "At the screen", value: summary.totals.screenMs)
                    Stat(label: "At the desk", value: summary.totals.deskMs)
                    Stat(label: "Drifted", value: summary.totals.driftMs)
                    Stat(label: "Away", value: summary.totals.awayMs)
                }
                .padding(.top, 16)
                .overlay(alignment: .top) { Hairline() }
                .padding(.top, 22)

                HStack(alignment: .top, spacing: 20) {
                    VStack(alignment: .leading, spacing: 0) {
                        Text("Where you drifted").font(Fonts.ui(12)).foregroundStyle(Palette.ink2).padding(.bottom, 6)
                        if summary.causes.isEmpty {
                            Text("Nowhere. Nice.").font(Fonts.ui(13)).foregroundStyle(Palette.ink3).padding(.top, 4)
                        } else {
                            CauseList(causes: summary.causes)
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)

                    let open = summary.unclear.filter { !$0.claimed && !dismissed.contains($0.segmentId) }
                    if let span = open.first {
                        UnclearReview(span: span, remaining: open.count) { worked in
                            summary = model.resolveUnclear(span, worked: worked, in: summary)
                            if !worked { dismissed.insert(span.segmentId) }
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                    }
                }
                .padding(.top, 20)

                HStack {
                    Spacer()
                    Button("Done") { WindowRouter.shared.close("summary") }.buttonStyle(QuietButtonStyle())
                }
                .padding(.top, 20)
            }
            .padding(.horizontal, 28)
            .padding(.top, 36)
            .padding(.bottom, 24)
    }

    private var header: String {
        let when = Fmt.partOfDay(summary.startedWall)
        if let label = summary.label { return "\(when) · \(label)" }
        return when
    }
}

struct Stat: View {
    let label: String
    let value: Int64
    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(label).font(Fonts.ui(11)).foregroundStyle(Palette.ink3)
            Text(Fmt.duration(value)).font(Fonts.serif(22)).foregroundStyle(Palette.ink)
        }
    }
}

struct TimeAxis: View {
    let start: Int64
    let end: Int64

    var body: some View {
        GeometryReader { geo in
            let ticks = Self.ticks(start: start, end: end)
            ZStack(alignment: .topLeading) {
                ForEach(ticks, id: \.self) { t in
                    let x = geo.size.width * CGFloat(Double(t - start) / Double(max(end - start, 1)))
                    Text(Fmt.time(t)).font(Fonts.ui(11)).foregroundStyle(Palette.ink3)
                        .fixedSize()
                        .position(x: min(max(x, 16), geo.size.width - 16), y: 7)
                }
            }
        }
        .frame(height: 14)
    }

    /// Round clock times inside the range, about five of them.
    static func ticks(start: Int64, end: Int64) -> [Int64] {
        let span = max(end - start, 60_000)
        let steps: [Int64] = [5, 10, 15, 30, 60, 120, 180].map { $0 * 60_000 }
        let step = steps.first { span / $0 <= 5 } ?? 360 * 60_000
        let tz = Int64(TimeZone.current.secondsFromGMT()) * 1000
        var t = ((start + tz) / step + 1) * step - tz
        var out: [Int64] = [start]
        while t < end - step / 3 {
            if t - start > step / 3 { out.append(t) }
            t += step
        }
        return out
    }
}

struct CauseList: View {
    let causes: [Cause]
    var body: some View {
        let top = causes.prefix(5)
        let maxMs = max(top.map(\.ms).max() ?? 1, 1)
        VStack(spacing: 0) {
            ForEach(Array(top.enumerated()), id: \.offset) { _, c in
                HStack(spacing: 10) {
                    Text(c.label).font(Fonts.ui(13)).foregroundStyle(Palette.ink).lineLimit(1)
                    Spacer(minLength: 8)
                    ZStack(alignment: .leading) {
                        RoundedRectangle(cornerRadius: 2).fill(Palette.sunk)
                        RoundedRectangle(cornerRadius: 2).fill(Palette.clay)
                            .frame(width: 80 * CGFloat(Double(c.ms) / Double(maxMs)))
                    }
                    .frame(width: 80, height: 6)
                    Text(Fmt.duration(c.ms)).font(Fonts.serif(13)).foregroundStyle(Palette.ink)
                        .lineLimit(1)
                        .fixedSize()
                        .frame(minWidth: 64, alignment: .trailing)
                }
                .padding(.vertical, 7)
                .overlay(alignment: .top) { Hairline() }
            }
            Hairline()
        }
    }
}

struct UnclearReview: View {
    let span: UnclearSpan
    let remaining: Int
    let answer: (Bool) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text("\(Fmt.duration(span.endWall - span.startWall)) we couldn't read")
                .font(Fonts.serif(17)).foregroundStyle(Palette.ink)
            Text(explanation).font(Fonts.ui(12)).foregroundStyle(Palette.ink2)
                .fixedSize(horizontal: false, vertical: true)
                .padding(.top, 4).padding(.bottom, 10)
            HStack(spacing: 6) {
                Button("Yes, count it") { answer(true) }.buttonStyle(InkButtonStyle())
                Button("No") { answer(false) }.buttonStyle(QuietButtonStyle(small: true))
                if remaining > 1 {
                    Spacer()
                    Text("1 of \(remaining)").font(Fonts.ui(11)).foregroundStyle(Palette.ink3)
                }
            }
        }
        .padding(.horizontal, 16).padding(.vertical, 14)
        .background(RoundedRectangle(cornerRadius: 10).fill(Palette.sunk))
    }

    private var explanation: String {
        let range = "\(Fmt.time(span.startWall)) to \(Fmt.time(span.endWall))."
        let what = span.atDesk
            ? "You were looking down and very still, with no book or pen in view."
            : "The screen was in front of you, but nothing moved for a long while."
        return "\(range) \(what) Were you working?"
    }
}
