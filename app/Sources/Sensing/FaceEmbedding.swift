import CoreImage
import CoreVideo
import Vision

/// Face embeddings for "is this you?": five Vision landmarks plus the frame
/// go to the SFace model in the Rust core. Only 128 numbers come back; the
/// frame is dropped as soon as the call returns.
final class SFaceEmbedder: FaceEmbedder, @unchecked Sendable {
    private let model: FaceModel
    private let ci = CIContext(options: [.useSoftwareRenderer: false])
    private let lock = NSLock()

    static var bundledModelURL: URL? {
        Bundle.main.url(forResource: "face_recognition_sface_2021dec", withExtension: "onnx")
    }

    init?() {
        guard let url = Self.bundledModelURL, let m = try? FaceModel(path: url.path) else { return nil }
        model = m
    }

    func embedding(_ pixel: CVPixelBuffer, face: VNFaceObservation) -> [Float]? {
        let w = CVPixelBufferGetWidth(pixel), h = CVPixelBufferGetHeight(pixel)
        guard let points = Self.fivePoints(face, width: w, height: h) else { return nil }
        lock.lock()
        defer { lock.unlock() }
        var rgba = [UInt8](repeating: 0, count: w * h * 4)
        ci.render(
            CIImage(cvPixelBuffer: pixel), toBitmap: &rgba, rowBytes: w * 4,
            bounds: CGRect(x: 0, y: 0, width: w, height: h), format: .RGBA8,
            colorSpace: CGColorSpace(name: CGColorSpace.sRGB)
        )
        return try? model.embed(rgba: Data(rgba), width: UInt32(w), height: UInt32(h), landmarks: points)
    }

    /// Eyes, nose tip and mouth corners in pixel coordinates (top-left
    /// origin), ordered left to right as they appear in the image.
    static func fivePoints(_ face: VNFaceObservation, width: Int, height: Int) -> [Point]? {
        guard let lm = face.landmarks else { return nil }
        let size = CGSize(width: width, height: height)
        func pts(_ r: VNFaceLandmarkRegion2D?) -> [CGPoint] { r?.pointsInImage(imageSize: size) ?? [] }
        func topLeft(_ p: CGPoint) -> Point { Point(x: Float(p.x), y: Float(CGFloat(height) - p.y)) }
        func center(_ ps: [CGPoint]) -> CGPoint? {
            ps.isEmpty ? nil : CGPoint(x: ps.map(\.x).reduce(0, +) / CGFloat(ps.count), y: ps.map(\.y).reduce(0, +) / CGFloat(ps.count))
        }

        guard let e1 = pts(lm.leftPupil).first ?? center(pts(lm.leftEye)),
              let e2 = pts(lm.rightPupil).first ?? center(pts(lm.rightEye)) else { return nil }
        let eyes = [e1, e2].sorted { $0.x < $1.x }

        // Nose tip: the lower part of the nose outline (y is up in Vision).
        let nose = pts(lm.nose)
        guard let minY = nose.map(\.y).min(), let maxY = nose.map(\.y).max(),
              let tip = center(nose.filter { $0.y <= minY + (maxY - minY) * 0.35 }) else { return nil }

        let lips = pts(lm.outerLips)
        guard let mouthL = lips.min(by: { $0.x < $1.x }), let mouthR = lips.max(by: { $0.x < $1.x }) else { return nil }

        return [topLeft(eyes[0]), topLeft(eyes[1]), topLeft(tip), topLeft(mouthL), topLeft(mouthR)]
    }
}
