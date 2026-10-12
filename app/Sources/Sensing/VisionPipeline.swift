import CoreImage
import CoreVideo
import Vision

/// Runs only the Vision requests the engine's sensing plan asks for this
/// tick. Everything runs through Apple's on-device models (Neural Engine).
/// Each instance is used from a single serial queue.
final class VisionPipeline: @unchecked Sendable {
    private let faceRequest: VNDetectFaceRectanglesRequest = {
        let r = VNDetectFaceRectanglesRequest()
        r.revision = VNDetectFaceRectanglesRequestRevision3
        return r
    }()
    private let landmarksRequest = VNDetectFaceLandmarksRequest()
    private let bodyRequest: VNDetectHumanRectanglesRequest = {
        let r = VNDetectHumanRectanglesRequest()
        r.upperBodyOnly = true
        return r
    }()
    private let handRequest: VNDetectHumanHandPoseRequest = {
        let r = VNDetectHumanHandPoseRequest()
        r.maximumHandCount = 2
        return r
    }()
    private let classifyRequest = VNClassifyImageRequest()
    private var lastHands: [Point] = []

    static let phoneLabels: Set<String> = ["phone"]
    static let studyLabels: Set<String> = ["book", "document", "newspaper", "sticky_note", "pen", "checkbook"]

    struct FaceOnly {
        var yaw: Float
        var pitch: Float
        var eyeOpen: Float?
        var faces: Int
    }

    /// Fills the camera fields of `frame`. Returns the main face observation
    /// (with landmarks) for identity checks.
    @discardableResult
    func analyze(_ pixel: CVPixelBuffer, plan: SensingPlan, into frame: inout SignalFrame) -> VNFaceObservation? {
        frame.brightness = Self.meanLuma(pixel)
        let handler = VNImageRequestHandler(cvPixelBuffer: pixel, orientation: .up, options: [:])

        try? handler.perform([faceRequest])
        let faces = faceRequest.results ?? []
        frame.faceCount = UInt32(faces.count)
        var main = faces.max { $0.boundingBox.width * $0.boundingBox.height < $1.boundingBox.width * $1.boundingBox.height }

        if let face = main {
            var eye: Float?
            if plan.landmarks || plan.verify {
                landmarksRequest.inputFaceObservations = [face]
                try? handler.perform([landmarksRequest])
                if let withLandmarks = landmarksRequest.results?.first {
                    main = withLandmarks
                    eye = Self.eyeOpenness(withLandmarks.landmarks)
                }
            }
            frame.face = FaceSig(
                yaw: face.yaw?.floatValue ?? 0,
                pitch: face.pitch?.floatValue ?? 0,
                roll: face.roll?.floatValue ?? 0,
                bbox: Rect(face.boundingBox),
                eyeOpen: eye
            )
        }

        if plan.body || main == nil {
            try? handler.perform([bodyRequest])
            frame.bodyPresent = (bodyRequest.results ?? []).contains { $0.confidence > 0.5 }
        }

        if plan.hands {
            try? handler.perform([handRequest])
            let hands: [Point] = (handRequest.results ?? []).compactMap { obs in
                guard let pts = try? obs.recognizedPoints(.all) else { return nil }
                let good = pts.values.filter { $0.confidence > 0.3 }
                guard !good.isEmpty else { return nil }
                let x = good.map { Float($0.location.x) }.reduce(0, +) / Float(good.count)
                let y = good.map { Float($0.location.y) }.reduce(0, +) / Float(good.count)
                return Point(x: x, y: y)
            }
            frame.hands = hands
            lastHands = hands
        }

        if plan.objects {
            frame.objects = detectObjects(handler)
        }
        return main
    }

    /// Phone and study material, by classifying small crops: around each
    /// hand for a phone, and the desk strip at the bottom for books/notes.
    private func detectObjects(_ handler: VNImageRequestHandler) -> ObjectsSig {
        var phone: Detection?
        let handROIs = lastHands.map { h in
            CGRect(x: CGFloat(h.x) - 0.16, y: CGFloat(h.y) - 0.16, width: 0.32, height: 0.32).intersection(CGRect(x: 0, y: 0, width: 1, height: 1))
        }.filter { $0.width > 0.05 && $0.height > 0.05 }
        // Without hands in view, only look at the bottom strip, where a phone
        // is most likely lying down rather than being held.
        let phoneROIs = handROIs.isEmpty ? [CGRect(x: 0, y: 0, width: 1, height: 0.3)] : handROIs
        for roi in phoneROIs {
            let conf = classify(handler, roi: roi, labels: Self.phoneLabels)
            if conf > (phone?.conf ?? 0) {
                phone = Detection(conf: conf, bbox: Rect(roi))
            }
        }
        let deskROI = CGRect(x: 0, y: 0, width: 1, height: 0.38)
        let bookConf = classify(handler, roi: deskROI, labels: Self.studyLabels)
        return ObjectsSig(phone: phone, book: Detection(conf: bookConf, bbox: Rect(deskROI)))
    }

    private func classify(_ handler: VNImageRequestHandler, roi: CGRect, labels: Set<String>) -> Float {
        classifyRequest.regionOfInterest = roi
        try? handler.perform([classifyRequest])
        return (classifyRequest.results ?? []).filter { labels.contains($0.identifier) }.map(\.confidence).max() ?? 0
    }

    struct EnrollmentFace {
        let face: VNFaceObservation
        let quality: Float
        let size: Float
        let yaw: Float
    }

    /// The main face with landmarks and Vision's capture-quality score,
    /// for enrollment.
    func enrollmentFace(_ pixel: CVPixelBuffer) -> EnrollmentFace? {
        let handler = VNImageRequestHandler(cvPixelBuffer: pixel, orientation: .up, options: [:])
        try? handler.perform([faceRequest])
        guard let face = (faceRequest.results ?? []).max(by: { $0.boundingBox.width < $1.boundingBox.width }) else { return nil }
        landmarksRequest.inputFaceObservations = [face]
        let quality = VNDetectFaceCaptureQualityRequest()
        quality.inputFaceObservations = [face]
        try? handler.perform([landmarksRequest, quality])
        guard let withLandmarks = landmarksRequest.results?.first else { return nil }
        return EnrollmentFace(
            face: withLandmarks,
            quality: quality.results?.first?.faceCaptureQuality ?? 0,
            size: Float(face.boundingBox.width),
            yaw: face.yaw?.floatValue ?? 0
        )
    }

    /// Face pose only, for onboarding calibration.
    func faceOnly(_ pixel: CVPixelBuffer) -> FaceOnly? {
        var frame = SignalFrame.empty(t: 0)
        let plan = SensingPlan(tickMs: 1000, landmarks: true, body: false, hands: false, objects: false, verify: false)
        analyze(pixel, plan: plan, into: &frame)
        guard let f = frame.face else { return FaceOnly(yaw: 0, pitch: 0, eyeOpen: nil, faces: 0) }
        return FaceOnly(yaw: f.yaw, pitch: f.pitch, eyeOpen: f.eyeOpen, faces: Int(frame.faceCount))
    }

    static func eyeOpenness(_ lm: VNFaceLandmarks2D?) -> Float? {
        func ratio(_ region: VNFaceLandmarkRegion2D?) -> Float? {
            guard let pts = region?.normalizedPoints, pts.count >= 4 else { return nil }
            let xs = pts.map(\.x), ys = pts.map(\.y)
            let w = (xs.max() ?? 0) - (xs.min() ?? 0)
            let h = (ys.max() ?? 0) - (ys.min() ?? 0)
            return w > 0 ? Float(h / w) : nil
        }
        let r = [ratio(lm?.leftEye), ratio(lm?.rightEye)].compactMap { $0 }
        return r.isEmpty ? nil : r.reduce(0, +) / Float(r.count)
    }

    /// Mean of the luma plane, sampled sparsely.
    static func meanLuma(_ px: CVPixelBuffer) -> Float {
        CVPixelBufferLockBaseAddress(px, .readOnly)
        defer { CVPixelBufferUnlockBaseAddress(px, .readOnly) }
        guard CVPixelBufferGetPlaneCount(px) > 0, let base = CVPixelBufferGetBaseAddressOfPlane(px, 0) else { return 0.5 }
        let w = CVPixelBufferGetWidthOfPlane(px, 0), h = CVPixelBufferGetHeightOfPlane(px, 0)
        let stride = CVPixelBufferGetBytesPerRowOfPlane(px, 0)
        let bytes = base.assumingMemoryBound(to: UInt8.self)
        var sum = 0, n = 0
        for y in Swift.stride(from: 0, to: h, by: 16) {
            for x in Swift.stride(from: 0, to: w, by: 16) {
                sum += Int(bytes[y * stride + x])
                n += 1
            }
        }
        return n > 0 ? Float(sum) / Float(n) / 255 : 0.5
    }
}

/// CVPixelBuffers are thread-safe to hand between queues; this just says so.
struct PixelBox: @unchecked Sendable {
    let buffer: CVPixelBuffer?
    init(_ buffer: CVPixelBuffer?) { self.buffer = buffer }
}

extension Rect {
    init(_ r: CGRect) {
        self.init(x: Float(r.origin.x), y: Float(r.origin.y), w: Float(r.width), h: Float(r.height))
    }
}

extension SignalFrame {
    static func empty(t: UInt64) -> SignalFrame {
        SignalFrame(
            tMs: t, wallMs: Clock.wallMs(), face: nil, faceCount: 0, isUser: nil, bodyPresent: nil, hands: nil,
            objects: nil, inputIdleS: 0, appBundle: nil, appName: nil, windowTitle: nil, urlHost: nil,
            screenLocked: false, cameraOk: false, brightness: 0
        )
    }
}
