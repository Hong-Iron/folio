import AVFoundation
import CoreVideo

/// The webcam at its lowest useful format and frame rate. Only the latest
/// frame is kept, in memory, and handed to Vision once per tick.
final class CameraSource: NSObject, AVCaptureVideoDataOutputSampleBufferDelegate {
    let session = AVCaptureSession()
    private let queue = DispatchQueue(label: "nd.camera", qos: .utility)
    private let lock = NSLock()
    private var latest: CVPixelBuffer?
    private var latestAt: UInt64 = 0
    private var configured = false
    private var wanted = false
    private var startedAt: UInt64 = 0
    private(set) var restarts = 0

    static var authorized: Bool { AVCaptureDevice.authorizationStatus(for: .video) == .authorized }
    static var denied: Bool {
        let s = AVCaptureDevice.authorizationStatus(for: .video)
        return s == .denied || s == .restricted
    }

    static func requestAccess(_ done: @escaping (Bool) -> Void) {
        AVCaptureDevice.requestAccess(for: .video) { ok in DispatchQueue.main.async { done(ok) } }
    }

    func start() {
        wanted = true
        startedAt = Clock.nowMs()
        queue.async { [self] in
            guard Self.authorized else { return }
            if !configured { configured = configure() }
            if configured && !session.isRunning { session.startRunning() }
        }
    }

    func stop() {
        wanted = false
        queue.async { [self] in
            if session.isRunning { session.stopRunning() }
            lock.lock()
            latest = nil
            lock.unlock()
        }
    }

    /// The newest frame, if it's fresh enough to describe "now".
    func latestFrame() -> CVPixelBuffer? {
        lock.lock()
        defer { lock.unlock() }
        guard let f = latest, Clock.nowMs() &- latestAt < 3_000 else { return nil }
        return f
    }

    var isHealthy: Bool {
        lock.lock()
        defer { lock.unlock() }
        return latest != nil && Clock.nowMs() &- latestAt < 3_000
    }

    /// Restarts capture when frames stop arriving (unplugged, taken by
    /// another app, or a stuck driver). Returns true if it had to act.
    @discardableResult
    func watchdog() -> Bool {
        guard wanted, Self.authorized else { return false }
        lock.lock()
        let last = max(latestAt, startedAt)
        lock.unlock()
        guard Clock.nowMs() &- last > 10_000 else { return false }
        restarts += 1
        startedAt = Clock.nowMs()
        queue.async { [self] in
            if session.isRunning { session.stopRunning() }
            session.beginConfiguration()
            session.inputs.forEach { session.removeInput($0) }
            session.outputs.forEach { session.removeOutput($0) }
            session.commitConfiguration()
            configured = configure()
            if configured { session.startRunning() }
        }
        return true
    }

    private func pickDevice() -> AVCaptureDevice? {
        let discovery = AVCaptureDevice.DiscoverySession(
            deviceTypes: [.builtInWideAngleCamera, .external, .continuityCamera],
            mediaType: .video,
            position: .unspecified
        )
        return discovery.devices.first { $0.deviceType == .builtInWideAngleCamera } ?? discovery.devices.first
            ?? AVCaptureDevice.default(for: .video)
    }

    private func configure() -> Bool {
        guard let device = pickDevice(), let input = try? AVCaptureDeviceInput(device: device) else { return false }
        session.beginConfiguration()
        if session.canSetSessionPreset(.vga640x480) {
            session.sessionPreset = .vga640x480
        } else if session.canSetSessionPreset(.low) {
            session.sessionPreset = .low
        }
        guard session.canAddInput(input) else {
            session.commitConfiguration()
            return false
        }
        session.addInput(input)
        let output = AVCaptureVideoDataOutput()
        output.videoSettings = [kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_420YpCbCr8BiPlanarFullRange]
        output.alwaysDiscardsLateVideoFrames = true
        output.setSampleBufferDelegate(self, queue: queue)
        guard session.canAddOutput(output) else {
            session.commitConfiguration()
            return false
        }
        session.addOutput(output)
        session.commitConfiguration()
        lowerFrameRate(device)
        return true
    }

    /// We look once a second, so ask the sensor for as few frames as it
    /// supports (but at least 2 fps so exposure keeps up).
    private func lowerFrameRate(_ device: AVCaptureDevice) {
        guard let ranges = Optional(device.activeFormat.videoSupportedFrameRateRanges), !ranges.isEmpty else { return }
        let minRate = ranges.map(\.minFrameRate).min() ?? 30
        let target = max(minRate, 2)
        guard ranges.contains(where: { $0.minFrameRate <= target && target <= $0.maxFrameRate }) else { return }
        do {
            try device.lockForConfiguration()
            let d = CMTime(value: 1000, timescale: CMTimeScale(target * 1000))
            device.activeVideoMinFrameDuration = d
            device.activeVideoMaxFrameDuration = d
            device.unlockForConfiguration()
        } catch {}
    }

    func captureOutput(_ output: AVCaptureOutput, didOutput sampleBuffer: CMSampleBuffer, from connection: AVCaptureConnection) {
        guard let px = CMSampleBufferGetImageBuffer(sampleBuffer) else { return }
        lock.lock()
        latest = px
        latestAt = Clock.nowMs()
        lock.unlock()
    }
}
