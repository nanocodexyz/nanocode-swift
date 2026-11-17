import Foundation
import Security

public struct WalletRecord: Codable, Sendable, Hashable {
  public let account: String
  public let key: String
  public let keyAddress: String
  public let expiry: Double
  public let authorization: JSON

  public var expired: Bool { expiry <= Date().timeIntervalSince1970 }

  init?(_ json: JSON) {
    guard let account = json["account"]?.string, let key = json["key"]?.string,
          let keyAddress = json["key_address"]?.string, let expiry = json["expiry"]?.number,
          let authorization = json["authorization"] else { return nil }
    self.account = account
    self.key = key
    self.keyAddress = keyAddress
    self.expiry = expiry
    self.authorization = authorization
  }

  var json: JSON {
    ["account": .string(account), "key": .string(key), "key_address": .string(keyAddress),
     "expiry": .number(expiry), "authorization": authorization]
  }
}

public struct ServiceSpend: Sendable, Hashable, Identifiable {
  public let host: String
  public let description: String?
  public let model: Bool
  public let symbol: String
  public let spent: String
  public let requests: Int
  public let last: String
  public var id: String { description.map { "\(host) · \($0)" } ?? host }
}

public struct PaymentStatus: Sendable, Hashable {
  public let account: String?
  public let key: String?
  public let source: String?
  public let total: String
  public let services: [ServiceSpend]

  public init(_ json: JSON) {
    account = json["wallet"]?["account"]?.string
    key = json["wallet"]?["key"]?.string
    source = json["wallet"]?["source"]?.string
    total = json["total"]?.string ?? "0.000000"
    services = (json["services"]?.array ?? []).map { service in
      ServiceSpend(
        host: service["host"]?.string ?? "", description: service["description"]?.string,
        model: service["model"]?.bool ?? false, symbol: service["symbol"]?.string ?? "",
        spent: service["spent"]?.string ?? "0", requests: service["requests"]?.int ?? 0,
        last: service["last"]?.string ?? "0")
    }
  }

  public var connected: Bool { account != nil }
}

public enum Keychain {
  public static func save(_ data: Data, service: String, account: String) throws {
    let query: [String: Any] = [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service,
                                kSecAttrAccount as String: account]
    SecItemDelete(query as CFDictionary)
    var item = query
    item[kSecValueData as String] = data
    item[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
    let status = SecItemAdd(item as CFDictionary, nil)
    guard status == errSecSuccess else { throw NanocodeError("Keychain refused to save (\(status))") }
  }

  public static func load(service: String, account: String) -> Data? {
    let query: [String: Any] = [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service,
                                kSecAttrAccount as String: account, kSecReturnData as String: true,
                                kSecMatchLimit as String: kSecMatchLimitOne]
    var result: AnyObject?
    guard SecItemCopyMatching(query as CFDictionary, &result) == errSecSuccess else { return nil }
    return result as? Data
  }

  public static func remove(service: String, account: String) {
    let query: [String: Any] = [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service,
                                kSecAttrAccount as String: account]
    SecItemDelete(query as CFDictionary)
  }
}

public struct Wallet: Sendable {
  public static let keychainService = "xyz.nanocode.wallet"
  let runtime: NanocodeRuntime

  public init(runtime: NanocodeRuntime) { self.runtime = runtime }

  public func restore() async -> PaymentStatus? {
    guard let data = Keychain.load(service: Self.keychainService, account: "access-key"),
          let json = try? JSON.parse(data), let record = WalletRecord(json) else { return nil }
    if record.expired {
      Keychain.remove(service: Self.keychainService, account: "access-key")
      return nil
    }
    guard let status = try? await runtime.command("wallet_use", record.json) else { return nil }
    return PaymentStatus(status)
  }

  /// Signs in with Tempo ID through its device grant: `name` is the device name Tempo ID's approval page
  /// shows, and `DevicePrompt` (`RuntimeEvent.walletPrompt`) carries the code the owner approves there.
  public func connect(name: String) async throws -> WalletRecord {
    let result = try await runtime.command("wallet_connect", ["name": .string(name)])
    guard let record = WalletRecord(result) else { throw NanocodeError("Tempo ID returned an unreadable access key") }
    try Keychain.save(record.json.data(), service: Self.keychainService, account: "access-key")
    return record
  }

  public func disconnect() async throws {
    Keychain.remove(service: Self.keychainService, account: "access-key")
    try await runtime.command("wallet_forget")
  }

  public func status() async throws -> PaymentStatus {
    PaymentStatus(try await runtime.command("payments"))
  }

  /// Tempo ID's deposit page for `account`, which offers Buy MACH and the other ways to add funds.
  public func fundingURL(account: String) async throws -> URL? {
    (try await runtime.command("funding_url", ["account": .string(account)])).string.flatMap(URL.init(string:))
  }
}
