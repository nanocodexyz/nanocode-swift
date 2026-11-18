# Local Apple release

Build, qualify and publish are separate operations. `publish-ffi.sh` never builds. It accepts an already qualified archive and a trusted local qualification record. It does not independently run the runtime checks recorded in that record. Existing workflows remain until a complete local replacement is available.

Use Xcode 26, XcodeGen, the Rust toolchain and targets in `rust-toolchain.toml`, Python 3.11 or newer, `gh`, `jq` and `curl`. Keep the SDK sibling at the revision in `sdk-revision.json` with no tracked changes. Verify the TypeScript release and shell bundle against `ts-revision.json` before qualification. Preserve the existing Rust wrappers and cache; do not clear wrapper variables or start a second build over the same outputs.

## Build and qualify

Run from the repository root with sufficient memory reserved for the operating system and other work. Wait for the entire script to finish, including XCFramework packaging, not just Cargo's exit.

```sh
set -euo pipefail
mkdir -p output
PROFILE=release UNIVERSAL=1 PLATFORMS='macos ios ios-sim' \
  CARGO_BUILD_JOBS=2 NUM_JOBS=2 SHIPPY_REMOTE_BUILD=0 \
  scripts/build-ffi.sh 2>&1 | tee output/ffi-build.log
swift build -j 2
NANOCODE_WITHOUT_FFI=1 swift build -j 2 --scratch-path output/without-ffi
swift build --package-path Examples/Console -j 2
console="$(swift build --package-path Examples/Console --show-bin-path)/Console"
"$console" bash 'printf "b\na\n" | sort | tr "\n" " "; echo "{\"n\": 2}" | jq .n; printf abc | sha256sum'
SDKROOT="$(xcrun --sdk macosx --show-sdk-path)" Examples/C/build.sh
SDKROOT="$(xcrun --sdk macosx --show-sdk-path)" Examples/Python/build.sh
xcodegen generate --spec Examples/Demo/project.yml
xcodebuild build -project Examples/Demo/NanocodeDemo.xcodeproj \
  -scheme NanocodeDemo-macOS -destination 'generic/platform=macOS' \
  -derivedDataPath output/DerivedData ARCHS=arm64 CODE_SIGNING_ALLOWED=NO -jobs 2
xcodebuild build -project Examples/Demo/NanocodeDemo.xcodeproj \
  -scheme NanocodeDemo-iOS -destination 'generic/platform=iOS Simulator' \
  -derivedDataPath output/DerivedData ARCHS=arm64 CODE_SIGNING_ALLOWED=NO -jobs 2
```

Require Console output `a b 2` followed by the SHA-256 digest of `abc`, and no XCFramework inside the no-FFI scratch directory. Save exit codes, commands and logs. Also require actual C ABI shell runs on macOS and the arm64 iOS simulator: basic Python, NumPy, platform defaults, Matplotlib Agg writing a valid PNG, and subprocess. An import check, mocked platform, successful link or demo build is not this runtime proof. No Python monkeypatch is permitted. Simulator proof is not physical-device proof.

Verify exactly three XCFramework entries: macOS arm64 and x86_64, iOS arm64 without a variant, and iOS arm64 with the simulator variant. Hash every packaged library against its linked `build/{macos,ios,ios-sim}` library. Thin each of the four architectures with `lipo -thin` and hash it against the corresponding `target/<triple>/dist/libnanocode_swift.a`. Require headers to match `include/`.

The November 18 qualification used ignored `output/epoch-6ed/ios-runtime-proof.py`, `final-guards.py`, `resume-qualification.sh` and `fresh-consumer.sh`, plus `output/ffi-shell-probe.py` and `output/ios-probe/ffi-cli.c`. Preserve these harnesses with their raw evidence when reproducing that release. They are not a checked-in qualification runner; the commands above alone do not establish every runtime gate.

## Seal and publish

Make a new artifact directory containing exactly the intended `NanocodeFFI.xcframework.zip` and `ffi-release.json`. Zip the qualified framework, not a rebuilt copy. The manifest has `url`, `checksum` and `fingerprint`; use `scripts/publish-ffi.sh --fingerprint`, SHA-256 of the zip, and the canonical release-download URL derived below:

```sh
mkdir -p output/release
ditto -c -k --sequesterRsrc --keepParent build/NanocodeFFI.xcframework output/release/NanocodeFFI.xcframework.zip
fingerprint=$(scripts/publish-ffi.sh --fingerprint)
checksum=$(shasum -a 256 output/release/NanocodeFFI.xcframework.zip | awk '{print $1}')
url="https://github.com/nanocodexyz/nanocode-swift/releases/download/ffi-$fingerprint/NanocodeFFI.xcframework.zip"
jq -n --arg url "$url" --arg checksum "$checksum" --arg fingerprint "$fingerprint" \
  '{url: $url, checksum: $checksum, fingerprint: $fingerprint}' >output/release/ffi-release.json
scripts/publish-ffi.sh --verify output/release output/qualification.json
scripts/publish-ffi.sh --upload output/release output/qualification.json
```

The qualification JSON contains:

- `fingerprint`, `checksum`, `core` and `ts`, identifying the archive and pinned revisions.
- `checks`, exactly these keys, each literal `true`: `swift`, `without_ffi`, `console`, `offline_shell`, `c`, `python`, `demo_macos`, `demo_ios`, `ios_runtime`, `macos_runtime`, `xcframework`.
- `source_hashes`, SHA-256 by relative path for `Cargo.toml`, `Cargo.lock`, both revision JSON files, `rust-toolchain.toml`, `scripts/build-ffi.sh`, `.cargo/config.toml`, `Package.swift`, and every file under `include/`, `src/` and `Sources/`.
- `libraries`, keyed by the Info.plist library identifiers. Each value has `platform`, `variant` (null or `simulator`), `architectures`, `sha256` and `bytes` for that packaged static library.

Only attest after reviewing the corresponding successful raw evidence. Hash bindings and a tracked-clean SDK checkout are not complete build provenance or a reproducible-build guarantee.

The publisher stages its inputs privately, validates those copies and uploads only those copies. An existing tag is never overwritten: both public assets must download and match exactly. Any uncertain create/upload outcome stops the script; inspect the remote release before another attempt. Its retained `output/ffi-publish.*` directory records verification and downloaded bytes. No automatic retry or rebuild occurs.

Before committing the resulting root `ffi-release.json`, move the local XCFramework aside with an EXIT trap that restores it, use a new SwiftPM scratch path, and run `env -u NANOCODE_WITHOUT_FFI swift build -j 2 --scratch-path <new-path>`. Verify the downloaded library's hash against the qualified macOS slice. Only then commit the metadata and intended source changes and push by normal fast-forward. Keep the exact zip, qualification JSON, runtime outputs and fresh-consumer evidence together.
