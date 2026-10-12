import SwiftUI

struct PopoverView: View {
    @EnvironmentObject var model: AppModel

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            if let problem = model.problem {
                ProblemBanner(text: problem) { model.problem = nil }
                    .padding(.bottom, 12)
            }
            if let i = model.interrupted, !model.sessionActive {
                InterruptedBanner(session: i).padding(.bottom, 12)
            }
            if model.sessionActive {
                SessionPane()
            } else {
                IdlePane()
            }
        }
        .padding(.horizontal, 18)
        .padding(.vertical, 16)
        .frame(width: 300)
        .background(Palette.raised)
    }
}

private struct IdlePane: View {
    @EnvironmentObject var model: AppModel
    @FocusState private var labelFocused: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(Fmt.longDate()).font(Fonts.ui(12)).foregroundStyle(Palette.ink3)
            Text(model.calibrated ? "Ready when you are." : "Let's get you set up.")
                .font(Fonts.serif(25))
                .foregroundStyle(Palette.ink)
                .padding(.top, 6)
            Text(model.todayFocusMs > 0 ? "\(Fmt.duration(model.todayFocusMs)) of focus so far today." : "No focus logged yet today.")
                .font(Fonts.ui(13))
                .foregroundStyle(Palette.ink2)
                .padding(.top, 2)

            if model.calibrated {
                Text("What are you working on?")
                    .font(Fonts.ui(12)).foregroundStyle(Palette.ink2)
                    .padding(.top, 16).padding(.bottom, 6)
                TextField("", text: $model.draftLabel, prompt: Text("Organic chemistry, ch. 4").foregroundStyle(Palette.ink3))
                    .textFieldStyle(.plain)
                    .font(Fonts.ui(13))
                    .foregroundStyle(Palette.ink)
                    .focused($labelFocused)
                    .padding(.horizontal, 10).padding(.vertical, 7)
                    .background(RoundedRectangle(cornerRadius: 7).fill(Palette.paper))
                    .overlay(RoundedRectangle(cornerRadius: 7).stroke(Palette.rule, lineWidth: 0.5))
                    .onSubmit { model.startSession() }

                HStack(spacing: 6) {
                    Chip(title: "45 min", selected: model.goalMin == 45) { model.goalMin = 45 }
                    Chip(title: "90 min", selected: model.goalMin == 90) { model.goalMin = 90 }
                    Chip(title: "No goal", selected: model.goalMin == nil) { model.goalMin = nil }
                }
                .padding(.top, 12).padding(.bottom, 18)

                HStack {
                    Button("Start session") { model.startSession() }.buttonStyle(InkButtonStyle())
                    Spacer()
                    Text("⌥⌘F").font(Fonts.ui(12)).foregroundStyle(Palette.ink3)
                }
            } else {
                Text("Two minutes: your camera, your face, and how you sit when you study.")
                    .font(Fonts.ui(13)).foregroundStyle(Palette.ink2)
                    .fixedSize(horizontal: false, vertical: true)
                    .padding(.top, 14).padding(.bottom, 16)
                Button("Set up") { WindowRouter.shared.showOnboarding() }.buttonStyle(InkButtonStyle())
            }

            Footer(trailing: "Camera off")
        }
    }
}

private struct SessionPane: View {
    @EnvironmentObject var model: AppModel

    var body: some View {
        let u = model.update
        VStack(alignment: .leading, spacing: 0) {
            Text(sessionTitle).font(Fonts.ui(12)).foregroundStyle(Palette.ink2)
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text(Fmt.clock(u?.focusMs ?? 0)).font(Fonts.serif(46))
                    .foregroundStyle(Palette.ink)
                Text("focused").font(Fonts.ui(13)).foregroundStyle(Palette.ink2)
            }
            .padding(.top, 2)
            Text(subline(u)).font(Fonts.ui(12)).foregroundStyle(Palette.ink3)
                .padding(.top, 2).padding(.bottom, 14)

            HStack(spacing: 8) {
                Dot(activity: u?.activity)
                Text(status(u)).font(Fonts.ui(13)).foregroundStyle(Palette.ink)
            }
            .padding(.bottom, 10)

            if !model.liveSegments.isEmpty {
                Ribbon(segments: model.liveSegments)
                HStack {
                    Text(Fmt.time(model.liveSegments.first?.startWall ?? 0))
                    Spacer()
                    Text("now")
                }
                .font(Fonts.ui(11)).foregroundStyle(Palette.ink3)
                .padding(.top, 4)
            }

            if let label = model.labelModeLabel {
                Text("Labeling: \(label)").font(Fonts.ui(11)).foregroundStyle(Palette.ink3).padding(.top, 8)
            }

            HStack(spacing: 8) {
                if u?.mode == .break {
                    Button("End break") { model.endBreak() }.buttonStyle(QuietButtonStyle())
                } else {
                    Menu {
                        Button("5 minutes") { model.takeBreak(minutes: 5) }
                        Button("10 minutes") { model.takeBreak(minutes: 10) }
                        Button("15 minutes") { model.takeBreak(minutes: 15) }
                    } label: {
                        Text("Take a break").font(Fonts.ui(13))
                    }
                    .menuStyle(.borderlessButton)
                    .menuIndicator(.hidden)
                    .fixedSize()
                    .padding(.horizontal, 12).padding(.vertical, 6)
                    .background(RoundedRectangle(cornerRadius: 7).fill(Palette.raised))
                    .overlay(RoundedRectangle(cornerRadius: 7).stroke(Palette.rule, lineWidth: 0.5))
                }
                Button("End session") { model.endSession() }.buttonStyle(QuietButtonStyle())
            }
            .padding(.top, 16)

            Footer(trailing: nil, camera: true)
        }
    }

    private var sessionTitle: String {
        model.draftLabel.trimmingCharacters(in: .whitespaces).isEmpty ? "Study session" : model.draftLabel
    }

    private func subline(_ u: EngineUpdate?) -> String {
        guard let u else { return "Starting the camera…" }
        var s = "of \(Fmt.clock(u.presentMs)) at your desk"
        if let goal = u.goalMs {
            s += u.focusMs >= goal ? " · \(goal / 60_000) min goal reached" : " · goal \(goal / 60_000) min"
        }
        return s
    }

    private func status(_ u: EngineUpdate?) -> String {
        guard let u else { return "Getting ready" }
        if u.mode == .break, let left = u.breakLeftMs {
            return "On a break, \(Fmt.clock(left + 59_999)) left"
        }
        if u.mode == .paused { return "Paused while you're away" }
        if u.notYou { return "Someone else is at your desk" }
        if u.tooDark { return "Too dark to see you clearly" }
        guard let a = u.activity else { return "Getting ready" }
        if a == .deskFocus { return "At your desk, studying" }
        return a.phrase(detail: u.detail)
    }
}

private struct Footer: View {
    var trailing: String?
    var camera = false

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Hairline().padding(.top, 16).padding(.bottom, 10)
            HStack(spacing: 16) {
                if camera {
                    Image(systemName: "camera").font(.system(size: 11)).foregroundStyle(Palette.ink3)
                    Text("Camera on. No images are saved.").font(Fonts.ui(12)).foregroundStyle(Palette.ink3)
                        .padding(.leading, -10)
                } else {
                    Button("Analytics") { WindowRouter.shared.showAnalytics() }.buttonStyle(LinkButtonStyle())
                    Button("Settings") { WindowRouter.shared.showSettings() }.buttonStyle(LinkButtonStyle())
                    Button("Quit") { NSApp.terminate(nil) }.buttonStyle(LinkButtonStyle())
                    Spacer()
                    if let trailing { Text(trailing).font(Fonts.ui(12)).foregroundStyle(Palette.ink3) }
                }
            }
        }
    }
}

private struct ProblemBanner: View {
    let text: String
    let dismiss: () -> Void

    var body: some View {
        HStack(alignment: .top, spacing: 8) {
            Circle().fill(Palette.clay).frame(width: 6, height: 6).padding(.top, 5)
            Text(text).font(Fonts.ui(12)).foregroundStyle(Palette.ink).fixedSize(horizontal: false, vertical: true)
            Spacer(minLength: 0)
            Button(action: dismiss) { Image(systemName: "xmark").font(.system(size: 10)) }
                .buttonStyle(.plain).foregroundStyle(Palette.ink3)
                .accessibilityLabel("Dismiss")
        }
        .padding(10)
        .background(RoundedRectangle(cornerRadius: 8).fill(Palette.sunk))
    }
}

private struct InterruptedBanner: View {
    @EnvironmentObject var model: AppModel
    let session: InterruptedSession

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Your \(Fmt.time(session.startedWall)) session was interrupted.")
                .font(Fonts.ui(12)).foregroundStyle(Palette.ink)
            HStack(spacing: 6) {
                Button("Resume") { model.resumeInterrupted() }.buttonStyle(QuietButtonStyle(small: true))
                Button("End it") { model.endInterrupted() }.buttonStyle(QuietButtonStyle(small: true))
            }
        }
        .padding(10)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: 8).fill(Palette.sunk))
    }
}
