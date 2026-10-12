import SwiftUI

enum AnalyticsRange: String, CaseIterable {
    case day = "Day"
    case week = "Week"
    case month = "Month"
}

struct AnalyticsView: View {
    @EnvironmentObject var model: AppModel
    @State private var range: AnalyticsRange
    @State private var anchor: Date
    @State private var report: RangeReport?
    @State private var error: String?

    init(range: AnalyticsRange = .week, anchor: Date = Date(), preloaded: RangeReport? = nil) {
        _range = State(initialValue: range)
        _anchor = State(initialValue: anchor)
        _report = State(initialValue: preloaded)
    }

    var body: some View {
        VStack(spacing: 0) {
            toolbar
            Hairline()
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    if let report {
                        content(report)
                    } else if let error {
                        Text(error).font(Fonts.ui(13)).foregroundStyle(Palette.ink2)
                    }
                }
                .padding(.horizontal, 24)
                .padding(.vertical, 20)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
        .background(Palette.paper)
        .onAppear { if report == nil { load() } }
        .onChange(of: range) { _, _ in load() }
        .onChange(of: anchor) { _, _ in load() }
    }

    private var toolbar: some View {
        HStack {
            Spacer().frame(width: 70)
            HStack(spacing: 2) {
                ForEach(AnalyticsRange.allCases, id: \.self) { r in
                    Button(r.rawValue) { range = r }
                        .buttonStyle(.plain)
                        .font(Fonts.ui(12))
                        .foregroundStyle(range == r ? Palette.ink : Palette.ink2)
                        .padding(.horizontal, 12).padding(.vertical, 3)
                        .background(RoundedRectangle(cornerRadius: 6).fill(range == r ? Palette.raised : Color.clear))
                }
            }
            .padding(2)
            .background(RoundedRectangle(cornerRadius: 8).fill(Palette.sunk))
            Spacer()
            HStack(spacing: 8) {
                Button { shift(-1) } label: { Image(systemName: "chevron.left") }.buttonStyle(.plain)
                    .accessibilityLabel("Previous")
                Text(rangeTitle).font(Fonts.ui(12)).foregroundStyle(Palette.ink2).frame(minWidth: 110)
                Button { shift(1) } label: { Image(systemName: "chevron.right") }.buttonStyle(.plain)
                    .accessibilityLabel("Next")
            }
            .foregroundStyle(Palette.ink2)
        }
        .padding(.horizontal, 14)
        .frame(height: 40)
    }

    // MARK: Data

    private var calendar: Calendar {
        var c = Calendar(identifier: .gregorian)
        c.firstWeekday = 2
        return c
    }

    private var period: (start: Date, days: Int) {
        let cal = calendar
        switch range {
        case .day:
            return (cal.startOfDay(for: anchor), 1)
        case .week:
            let start = cal.dateInterval(of: .weekOfYear, for: anchor)?.start ?? anchor
            return (start, 7)
        case .month:
            let interval = cal.dateInterval(of: .month, for: anchor)
            let start = interval?.start ?? anchor
            let days = cal.range(of: .day, in: .month, for: anchor)?.count ?? 30
            return (start, days)
        }
    }

    private var rangeTitle: String {
        let f = DateFormatter()
        let p = period
        switch range {
        case .day:
            f.dateFormat = "EEE, MMM d"
            return f.string(from: p.start)
        case .week:
            f.dateFormat = "MMM d"
            let end = calendar.date(byAdding: .day, value: 6, to: p.start) ?? p.start
            let g = DateFormatter()
            g.dateFormat = calendar.isDate(p.start, equalTo: end, toGranularity: .month) ? "d" : "MMM d"
            return "\(f.string(from: p.start)) to \(g.string(from: end))"
        case .month:
            f.dateFormat = "MMMM yyyy"
            return f.string(from: p.start)
        }
    }

    private func shift(_ n: Int) {
        let unit: Calendar.Component = range == .day ? .day : range == .week ? .weekOfYear : .month
        anchor = calendar.date(byAdding: unit, value: n, to: anchor) ?? anchor
    }

    private func load() {
        let p = period
        do {
            report = try model.engine.report(fromDate: Clock.dateString(p.start), days: UInt32(p.days))
            error = nil
        } catch {
            self.error = "Couldn't load your history: \(error.localizedDescription)"
        }
    }

    // MARK: Content

    @ViewBuilder
    private func content(_ r: RangeReport) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 10) {
            Text(Fmt.duration(r.totals.focusMs)).font(Fonts.serif(38)).foregroundStyle(Palette.ink)
            Text(comparison(r)).font(Fonts.ui(14)).foregroundStyle(Palette.ink2)
        }

        if r.totals.focusMs == 0 && r.totals.driftMs == 0 {
            Text(range == .day ? "Nothing recorded on this day." : "Nothing recorded in this period yet. Start a session from the menu bar.")
                .font(Fonts.ui(13)).foregroundStyle(Palette.ink3).padding(.top, 12)
        } else {
            switch range {
            case .day: DayDetail(report: r)
            case .week: WeekLedger(report: r)
            case .month: MonthCalendar(report: r, start: period.start)
            }

            HStack(alignment: .top, spacing: 24) {
                VStack(alignment: .leading, spacing: 10) {
                    Text("When you focus").font(Fonts.ui(12)).foregroundStyle(Palette.ink2)
                    HourHistogram(hours: r.hours)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                VStack(alignment: .leading, spacing: 6) {
                    Text("What pulled you away").font(Fonts.ui(12)).foregroundStyle(Palette.ink2)
                    if r.causes.isEmpty {
                        Text("Nothing. Nice.").font(Fonts.ui(13)).foregroundStyle(Palette.ink3).padding(.top, 4)
                    } else {
                        CauseList(causes: r.causes)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .padding(.top, 16)
            .overlay(alignment: .top) { Hairline() }
            .padding(.top, 22)

            if !r.insights.isEmpty {
                VStack(alignment: .leading, spacing: 4) {
                    ForEach(Array(r.insights.enumerated()), id: \.offset) { i, s in
                        Text(s).font(Fonts.serif(14)).foregroundStyle(i == 0 ? Palette.ink : Palette.ink2)
                    }
                }
                .padding(.top, 14)
                .overlay(alignment: .top) { Hairline() }
                .padding(.top, 20)
            }

            HStack(spacing: 16) {
                Text("Screen \(Fmt.duration(r.totals.screenMs))")
                Text("Desk \(Fmt.duration(r.totals.deskMs))")
                Text("Drifted \(Fmt.duration(r.totals.driftMs))")
                if r.totals.unclearMs > 0 { Text("Unclear \(Fmt.duration(r.totals.unclearMs))") }
            }
            .font(Fonts.ui(12)).foregroundStyle(Palette.ink3)
            .padding(.top, 18)
        }
    }

    private func comparison(_ r: RangeReport) -> String {
        let noun = range == .day ? "today" : range == .week ? "this week" : "this month"
        let prevNoun = range == .day ? "the day before" : range == .week ? "last week" : "last month"
        let label = range == .day && !calendar.isDateInToday(anchor) ? "focused" : "focused \(noun)"
        guard r.prevFocusMs > 0 else { return "\(label)." }
        let diff = r.totals.focusMs - r.prevFocusMs
        if abs(diff) < 60_000 { return "\(label), about the same as \(prevNoun)." }
        return "\(label), \(Fmt.duration(abs(diff))) \(diff > 0 ? "more" : "less") than \(prevNoun)."
    }
}

// MARK: - Pieces

private let dayStartMin: UInt32 = 6 * 60
private let daySpanMin: Double = 18 * 60

struct DayStrip: View {
    let pieces: [RibbonPiece]
    var height: CGFloat = 14

    var body: some View {
        GeometryReader { geo in
            ZStack(alignment: .leading) {
                RoundedRectangle(cornerRadius: 3).fill(Palette.sunk)
                ForEach([9, 12, 15, 18, 21], id: \.self) { h in
                    Rectangle().fill(Palette.rule).frame(width: 0.5, height: height + 6)
                        .offset(x: geo.size.width * CGFloat(Double(h * 60 - Int(dayStartMin)) / daySpanMin))
                }
                ForEach(Array(pieces.enumerated()), id: \.offset) { _, p in
                    let s = max(Double(p.startMin) - Double(dayStartMin), 0)
                    let e = min(Double(p.endMin) - Double(dayStartMin), daySpanMin)
                    if e > s {
                        piece(p.activity)
                            .frame(width: max(geo.size.width * CGFloat((e - s) / daySpanMin) - 1, 1), height: height)
                            .offset(x: geo.size.width * CGFloat(s / daySpanMin))
                    }
                }
            }
        }
        .frame(height: height)
    }

    @ViewBuilder
    private func piece(_ a: Activity) -> some View {
        if a.dashed {
            RoundedRectangle(cornerRadius: 2).strokeBorder(Palette.ink3, style: StrokeStyle(lineWidth: 1, dash: [3, 2]))
        } else {
            RoundedRectangle(cornerRadius: 2).fill(a.color)
        }
    }
}

struct HourAxis: View {
    var body: some View {
        GeometryReader { geo in
            ForEach([6, 9, 12, 15, 18, 21, 24], id: \.self) { h in
                let x = geo.size.width * CGFloat(Double(h - 6) / 18)
                Text("\(h):00").font(Fonts.ui(11)).foregroundStyle(Palette.ink3).fixedSize()
                    .position(x: min(max(x, 14), geo.size.width - 16), y: 7)
            }
        }
        .frame(height: 14)
    }
}

private struct WeekLedger: View {
    let report: RangeReport
    private let names = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]

    var body: some View {
        VStack(spacing: 0) {
            ForEach(Array(report.days.enumerated()), id: \.offset) { _, d in
                HStack(spacing: 10) {
                    Text(names[Int(d.weekday) % 7]).font(Fonts.ui(12)).foregroundStyle(Palette.ink2).frame(width: 34, alignment: .leading)
                    DayStrip(pieces: d.pieces)
                    Text(Fmt.clock(UInt64(max(d.totals.focusMs, 0)))).font(Fonts.serif(15)).foregroundStyle(Palette.ink)
                        .frame(width: 44, alignment: .trailing)
                }
                .frame(height: 26)
            }
            HStack(spacing: 10) {
                Spacer().frame(width: 34)
                HourAxis()
                Spacer().frame(width: 44)
            }
            Legend(kinds: [.screenFocus, .deskFocus, .social, .away]).frame(maxWidth: .infinity, alignment: .leading).padding(.top, 8)
        }
        .padding(.top, 18)
    }
}

private struct DayDetail: View {
    let report: RangeReport

    var body: some View {
        let d = report.days.first
        VStack(alignment: .leading, spacing: 0) {
            DayStrip(pieces: d?.pieces ?? [], height: 22)
            HourAxis().padding(.top, 4)
            Legend().padding(.top, 8)
            LazyVGrid(columns: Array(repeating: GridItem(.flexible(), alignment: .leading), count: 4), spacing: 12) {
                Stat(label: "Longest stretch", value: d?.longestMs ?? 0)
                Stat(label: "At the screen", value: d?.totals.screenMs ?? 0)
                Stat(label: "At the desk", value: d?.totals.deskMs ?? 0)
                VStack(alignment: .leading, spacing: 2) {
                    Text("Drifts").font(Fonts.ui(11)).foregroundStyle(Palette.ink3)
                    Text("\(d?.drifts ?? 0)").font(Fonts.serif(22)).foregroundStyle(Palette.ink)
                }
            }
            .padding(.top, 18)
        }
        .padding(.top, 18)
    }
}

private struct MonthCalendar: View {
    let report: RangeReport
    let start: Date

    var body: some View {
        let lead = Int(report.days.first?.weekday ?? 0)
        let maxFocus = max(report.days.map(\.totals.focusMs).max() ?? 1, 1)
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 0) {
                ForEach(["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"], id: \.self) { n in
                    Text(n).font(Fonts.ui(11)).foregroundStyle(Palette.ink3).frame(maxWidth: .infinity)
                }
            }
            LazyVGrid(columns: Array(repeating: GridItem(.flexible(), spacing: 0), count: 7), spacing: 6) {
                ForEach(0..<lead, id: \.self) { _ in Color.clear.frame(height: 54) }
                ForEach(Array(report.days.enumerated()), id: \.offset) { i, d in
                    VStack(spacing: 4) {
                        ZStack {
                            Circle().stroke(Palette.rule, lineWidth: 0.5).frame(width: 30, height: 30)
                            if d.totals.focusMs > 0 {
                                let r = 8 + 22 * sqrt(Double(d.totals.focusMs) / Double(maxFocus))
                                Circle().fill(Palette.moss).frame(width: r, height: r)
                            }
                        }
                        Text("\(i + 1)").font(Fonts.ui(11)).foregroundStyle(Palette.ink3)
                    }
                    .frame(height: 54)
                    .help(d.totals.focusMs > 0 ? "\(Fmt.duration(d.totals.focusMs)) focused" : "No focus")
                }
            }
        }
        .padding(.top, 18)
    }
}

struct HourHistogram: View {
    let hours: [Int64]

    var body: some View {
        let visible = Array(hours.dropFirst(6))
        let maxV = max(visible.max() ?? 1, 1)
        VStack(spacing: 3) {
            HStack(alignment: .bottom, spacing: 2) {
                ForEach(Array(visible.enumerated()), id: \.offset) { i, v in
                    UnevenRoundedRectangle(topLeadingRadius: 2, topTrailingRadius: 2)
                        .fill(Palette.moss)
                        .frame(height: v > 0 ? max(3, 66 * CGFloat(Double(v) / Double(maxV))) : 0)
                        .frame(maxWidth: .infinity)
                        .help("\(i + 6):00, \(Fmt.duration(v))")
                }
            }
            .frame(height: 70, alignment: .bottom)
            .overlay(alignment: .bottom) { Rectangle().fill(Palette.ink3).frame(height: 0.5) }
            HStack {
                Text("6:00")
                Spacer()
                Text("12:00")
                Spacer()
                Text("18:00")
                Spacer()
                Text("24:00")
            }
            .font(Fonts.ui(11)).foregroundStyle(Palette.ink3)
        }
    }
}
