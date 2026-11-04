import AVFoundation
import SwiftUI

/// Scans the pairing QR shown in Desktop settings. The URL only fills the editor; it is never
/// opened or connected automatically.
struct QRScannerSheet: View {
    let onScanned: (ScannedConnection) -> Void
    let onManual: () -> Void
    @State private var authorization = AVCaptureDevice.authorizationStatus(for: .video)
    @State private var invalid = false
    @State private var cameraFailed = false

    var body: some View {
        NavigationStack {
            ZStack(alignment: .bottom) {
                Color.black.ignoresSafeArea()
                switch authorization {
                case .authorized:
                    QRCameraView(onCode: { text in
                        guard let scanned = try? ConnectionQr.parse(text) else { invalid = true; return false }
                        UINotificationFeedbackGenerator().notificationOccurred(.success)
                        onScanned(scanned)
                        return true
                    }, onFailure: { cameraFailed = true })
                    .ignoresSafeArea()
                    RoundedRectangle(cornerRadius: 24).stroke(.white.opacity(0.85), lineWidth: 3)
                        .frame(width: 240, height: 240)
                        .frame(maxHeight: .infinity)
                case .notDetermined:
                    ProgressView().tint(.white)
                default:
                    ContentUnavailableView {
                        Label(L.t("qr_camera_permission"), systemImage: "camera")
                    } description: {
                        Text(L.t("ios_scan_permission_denied"))
                    } actions: {
                        Button(L.t("qr_open_settings")) {
                            if let url = URL(string: UIApplication.openSettingsURLString) { UIApplication.shared.open(url) }
                        }
                        .buttonStyle(.borderedProminent)
                    }
                    .foregroundStyle(.white)
                }
                VStack(spacing: 8) {
                    Text(L.t("qr_scan_hint")).font(.callout)
                    if invalid || cameraFailed {
                        Text(L.t(cameraFailed ? "qr_camera_failed" : "qr_scan_invalid")).font(.footnote).foregroundStyle(.red)
                    }
                    Button(L.t("qr_scan_manual"), action: onManual).buttonStyle(.bordered)
                }
                .multilineTextAlignment(.center)
                .padding()
                .frame(maxWidth: .infinity)
                .background(.regularMaterial)
            }
            .navigationTitle(L.t("qr_scan_title"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbarBackground(.visible, for: .navigationBar)
            .toolbar { ToolbarItem(placement: .cancellationAction) { Button(L.t("action_cancel"), action: onManual) } }
            .task {
                if authorization == .notDetermined {
                    _ = await AVCaptureDevice.requestAccess(for: .video)
                    authorization = AVCaptureDevice.authorizationStatus(for: .video)
                }
            }
        }
    }
}

private struct QRCameraView: UIViewControllerRepresentable {
    let onCode: (String) -> Bool
    let onFailure: () -> Void

    func makeUIViewController(context: Context) -> QRCameraController {
        let controller = QRCameraController()
        controller.onCode = onCode
        controller.onFailure = onFailure
        return controller
    }

    func updateUIViewController(_ controller: QRCameraController, context: Context) {
        controller.onCode = onCode
        controller.onFailure = onFailure
    }
}

final class QRCameraController: UIViewController, AVCaptureMetadataOutputObjectsDelegate {
    var onCode: (String) -> Bool = { _ in false }
    var onFailure: () -> Void = {}
    private let session = AVCaptureSession()
    private var preview: AVCaptureVideoPreviewLayer?
    private var consumed = false
    private var lastRejected: String?

    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = .black
        guard let device = AVCaptureDevice.default(for: .video), let input = try? AVCaptureDeviceInput(device: device),
              session.canAddInput(input) else { onFailure(); return }
        session.addInput(input)
        let output = AVCaptureMetadataOutput()
        guard session.canAddOutput(output) else { onFailure(); return }
        session.addOutput(output)
        output.setMetadataObjectsDelegate(self, queue: .main)
        output.metadataObjectTypes = [.qr]
        let layer = AVCaptureVideoPreviewLayer(session: session)
        layer.videoGravity = .resizeAspectFill
        view.layer.addSublayer(layer)
        preview = layer
    }

    override func viewDidLayoutSubviews() {
        super.viewDidLayoutSubviews()
        preview?.frame = view.bounds
    }

    override func viewWillAppear(_ animated: Bool) {
        super.viewWillAppear(animated)
        let session = session
        DispatchQueue.global(qos: .userInitiated).async { if !session.isRunning { session.startRunning() } }
    }

    override func viewWillDisappear(_ animated: Bool) {
        super.viewWillDisappear(animated)
        let session = session
        DispatchQueue.global(qos: .userInitiated).async { session.stopRunning() }
    }

    func metadataOutput(_ output: AVCaptureMetadataOutput, didOutput objects: [AVMetadataObject], from connection: AVCaptureConnection) {
        guard !consumed, let text = (objects.first as? AVMetadataMachineReadableCodeObject)?.stringValue, text != lastRejected else { return }
        if onCode(text) {
            consumed = true
            let session = session
            DispatchQueue.global(qos: .userInitiated).async { session.stopRunning() }
        } else {
            lastRejected = text
        }
    }
}
