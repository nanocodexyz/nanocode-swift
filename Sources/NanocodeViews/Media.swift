import SwiftUI
#if os(macOS)
import AppKit
public typealias PlatformImage = NSImage
#else
import UIKit
public typealias PlatformImage = UIImage
#endif

private struct ImagesKey: EnvironmentKey {
  static let defaultValue: (any ImageResolver)? = nil
}

public extension EnvironmentValues {
  var nanocodeImages: (any ImageResolver)? {
    get { self[ImagesKey.self] }
    set { self[ImagesKey.self] = newValue }
  }
}

public struct RemoteImage: View {
  let url: String
  var maxHeight: CGFloat
  @Environment(\.nanocodeTheme) private var theme
  @Environment(\.nanocodeImages) private var resolver
  @State private var resolved: PlatformImage?
  @State private var failed = false

  public init(_ url: String, maxHeight: CGFloat = 320) {
    self.url = url
    self.maxHeight = maxHeight
  }

  public var body: some View {
    Group {
      if let data = DataURL.decode(url), let image = PlatformImage(data: data) {
        platform(image).resizable().scaledToFit()
      } else if let resolved {
        platform(resolved).resizable().scaledToFit()
      } else if url.hasPrefix("/"), FileManager.default.fileExists(atPath: url), let image = PlatformImage(contentsOfFile: url) {
        platform(image).resizable().scaledToFit()
      } else if resolver != nil, !failed {
        ProgressView()
      } else if let link = URL(string: url), link.scheme?.hasPrefix("http") == true {
        AsyncImage(url: link) { phase in
          if let image = phase.image { image.resizable().scaledToFit() } else { ProgressView() }
        }
      } else {
        Label("image", systemImage: "photo").foregroundStyle(theme.muted)
      }
    }
    .frame(maxHeight: maxHeight)
    .clipShape(RoundedRectangle(cornerRadius: 8))
    .task(id: url) {
      guard let resolver, DataURL.decode(url) == nil, !(url.hasPrefix("/") && FileManager.default.fileExists(atPath: url)) else { return }
      resolved = (try? await resolver.imageData(url)).flatMap(PlatformImage.init(data:))
      failed = resolved == nil
    }
  }

  func platform(_ image: PlatformImage) -> Image {
    #if os(macOS)
    Image(nsImage: image)
    #else
    Image(uiImage: image)
    #endif
  }
}
