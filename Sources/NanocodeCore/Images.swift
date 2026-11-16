import Foundation

public protocol ImageResolver: Sendable {
  func imageData(_ source: String) async throws -> Data
}

public enum ImageSource {
  public static func blob(_ source: String) -> (type: String, key: String)? {
    let value = source.trimmingCharacters(in: .whitespaces)
    guard value.hasPrefix("nanocode-blob:"), let semicolon = value.firstIndex(of: ";") else { return nil }
    let type = String(value[value.index(value.startIndex, offsetBy: 14)..<semicolon])
    let key = String(value[value.index(after: semicolon)...])
    guard type.hasPrefix("image/"), !type.contains(","), key.hasPrefix("b3:"), key.count == 67,
          key.dropFirst(3).allSatisfy({ $0.isHexDigit && !$0.isUppercase }) else { return nil }
    return (type, key)
  }

  public static func workspacePath(_ source: String, root: String = "/workspace") -> String? {
    let value = source.trimmingCharacters(in: .whitespaces)
    var found: String?
    for scheme in ["workspace://", "workspace:", "sandbox://", "sandbox:"] where value.lowercased().hasPrefix(scheme) {
      found = String(value.dropFirst(scheme.count))
      break
    }
    if found == nil, value.lowercased().hasPrefix("file:///") { found = String(value.dropFirst(7)) }
    if found == nil, value.hasPrefix(root + "/") { found = value }
    guard let found else { return nil }
    let raw = found.removingPercentEncoding ?? found
    let absolute = raw.hasPrefix("/") ? raw : "/" + raw
    let resolved = absolute == root || absolute.hasPrefix(root + "/") ? absolute : root + absolute
    guard !resolved.split(separator: "/").contains("..") else { return nil }
    return String(resolved.prefix { $0 != "?" && $0 != "#" })
  }

  public static func isImage(_ source: String) -> Bool {
    let value = source.trimmingCharacters(in: .whitespaces).lowercased()
    if value.hasPrefix("http://") || value.hasPrefix("https://") { return true }
    for kind in ["png", "jpeg", "jpg", "gif", "webp", "avif", "bmp"] where value.hasPrefix("data:image/\(kind)") { return true }
    return blob(source) != nil || workspacePath(source) != nil
  }
}

public struct WorkspaceImages: ImageResolver {
  public var directory: URL
  public var root: String

  public init(directory: URL, root: String = "/workspace") {
    self.directory = directory
    self.root = root
  }

  public func imageData(_ source: String) async throws -> Data {
    if let data = DataURL.decode(source) { return data }
    guard let path = ImageSource.workspacePath(source, root: root) else { throw URLError(.unsupportedURL) }
    let relative = path.dropFirst(root.count).drop { $0 == "/" }
    return try Data(contentsOf: relative.isEmpty ? directory : directory.appendingPathComponent(String(relative)))
  }
}
