import AppKit
import CoreImage

/// Label mode, for measuring accuracy on your own setup. While it's on, each
/// session writes a signal trace (JSONL, no images) plus the labels you mark
/// with hotkeys, in the format `nd-replay` reads. Thumbnails are a separate
/// opt-in and are deleted automatically after 14 days.
final class LabelRecorder: @unchecked Sendable {
    static let labels: [(key: String, label: String, title: String)] = [
        ("1", "screen", "At the screen"),
        ("2", "desk", "At the desk"),
        ("3", "phone", "On the phone"),
        ("4", "social", "Social media"),
        ("5", "idle", "Doing nothing"),
        ("6", "away", "Away"),
    ]

    // All file state below is touched only on `queue`.
    private let queue = DispatchQueue(label: "nd.recorder", qos: .utility)
    private var trace: FileHandle?
    private var labelsFile: FileHandle?
    private var thumbsDir: URL?
    private var current: (label: String, since: UInt64)?
    private let ci = CIContext(options: [.useSoftwareRenderer: false])
    /// Main-thread copies for the UI.
    private(set) var currentLabel: String?
    private(set) var recording = false

    var enabled: Bool {
        get { UserDefaults.standard.bool(forKey: "labelMode") }
        set { UserDefaults.standard.set(newValue, forKey: "labelMode") }
    }

    var keepThumbnails: Bool {
        get { UserDefaults.standard.bool(forKey: "labelThumbnails") }
        set { UserDefaults.standard.set(newValue, forKey: "labelThumbnails") }
    }

    static var folder: URL {
        AppPaths.support.appendingPathComponent("Traces", isDirectory: true)
    }

    func sessionStarted(calibration: Calibration) {
        guard enabled else { return }
        recording = true
        let header = calibrationToJson(cal: calibration) + "\n"
        let thumbs = keepThumbnails
        queue.async { self.open(header: header, thumbnails: thumbs) }
    }

    private func open(header: String, thumbnails: Bool) {
        let fm = FileManager.default
        try? fm.createDirectory(at: Self.folder, withIntermediateDirectories: true)
        let f = DateFormatter()
        f.dateFormat = "yyyy-MM-dd-HHmmss"
        let stem = f.string(from: Date())
        let traceURL = Self.folder.appendingPathComponent("\(stem).jsonl")
        let labelsURL = Self.folder.appendingPathComponent("\(stem).labels.csv")
        fm.createFile(atPath: traceURL.path, contents: Data(header.utf8))
        fm.createFile(atPath: labelsURL.path, contents: Data("start_ms,end_ms,label\n".utf8))
        trace = try? FileHandle(forWritingTo: traceURL)
        trace?.seekToEndOfFile()
        labelsFile = try? FileHandle(forWritingTo: labelsURL)
        labelsFile?.seekToEndOfFile()
        if thumbnails {
            let dir = Self.folder.appendingPathComponent("\(stem)-thumbs", isDirectory: true)
            try? fm.createDirectory(at: dir, withIntermediateDirectories: true)
            thumbsDir = dir
        }
    }

    /// Called on the sensing queue once per tick.
    func record(_ frame: SignalFrame, pixel: CVPixelBuffer?) {
        let box = PixelBox(pixel)
        queue.async { [self] in
            guard let trace else { return }
            trace.write(Data((frameToJson(frame: frame) + "\n").utf8))
            if let dir = thumbsDir, let pixel = box.buffer {
                let image = CIImage(cvPixelBuffer: pixel)
                let scale = 160 / max(image.extent.width, 1)
                let small = image.transformed(by: CGAffineTransform(scaleX: scale, y: scale))
                if let cs = CGColorSpace(name: CGColorSpace.sRGB),
                   let data = ci.jpegRepresentation(of: small, colorSpace: cs, options: [kCGImageDestinationLossyCompressionQuality as CIImageRepresentationOption: 0.5]) {
                    try? data.write(to: dir.appendingPathComponent("\(frame.tMs).jpg"))
                }
            }
        }
    }

    func mark(_ label: String, at t: UInt64) {
        guard recording else { return }
        currentLabel = label
        queue.async { [self] in
            closeSpan(at: t)
            current = (label, t)
        }
    }

    private func closeSpan(at t: UInt64) {
        guard let c = current, let labelsFile, t > c.since else { return }
        labelsFile.write(Data("\(c.since),\(t),\(c.label)\n".utf8))
        current = nil
    }

    func sessionEnded(at t: UInt64) {
        currentLabel = nil
        recording = false
        queue.sync {
            closeSpan(at: t)
            try? trace?.close()
            try? labelsFile?.close()
            trace = nil
            labelsFile = nil
            thumbsDir = nil
        }
    }

    static func pruneOldThumbnails() {
        let fm = FileManager.default
        guard let items = try? fm.contentsOfDirectory(at: folder, includingPropertiesForKeys: [.creationDateKey]) else { return }
        let cutoff = Date().addingTimeInterval(-14 * 86_400)
        for url in items where url.lastPathComponent.hasSuffix("-thumbs") {
            let created = (try? url.resourceValues(forKeys: [.creationDateKey]).creationDate) ?? Date()
            if created < cutoff { try? fm.removeItem(at: url) }
        }
    }

    static func deleteAll() {
        try? FileManager.default.removeItem(at: folder)
    }
}

enum AppPaths {
    static let support: URL = {
        let dir = base.appendingPathComponent("Folio", isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir
    }()

    static var base: URL {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
    }
}
