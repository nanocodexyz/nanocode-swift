# Local workflow qualification

## Source-only integration status

These scripts require no checked-in vendor tree. They do require installed toolchains, local locked Cargo sources and an already qualified XCFramework.
The source-only change does not make a clean checkout self-contained for offline qualification. Missing local inputs fail closed.
The workflow replacement passed at Swift 033ae35dde78ce26c8876cb783cbd7106934ef18 with core 6ed92be87b3a2f704a7af857a2237b3fccafbb27.
Apple package/examples/demo checks and the Linux Cargo check passed with OS networking denied, followed by the copied-evidence combined gate.
Independent review approved removal of only the existing CI workflow. This is not a new 7d4 FFI or iOS runtime qualification.

Keep the existing workflow until both platform modes pass and their copied evidence bundles pass `gate`. This runner covers the workflow's package, no-FFI, Console, C/Python, two demo builds and Linux Cargo checks. It is not a full FFI rebuild, publisher qualification or new iOS runtime attestation.

## Apple

Use macOS with Xcode 26, XcodeGen 2.46.0, Python 3.11 or newer and the already qualified local XCFramework in `build/NanocodeFFI.xcframework`. Supply the sealed release directory and its trusted qualification JSON:

```sh
scripts/reference-swift.sh apple /absolute/release-directory /absolute/qualification.json 2
```

The no-build publisher checks the sealed artifact. The runner compares local libraries and headers and never downloads or builds FFI. An outer Seatbelt sandbox denies IP networking while preserving local build IPC. SwiftPM receives `--disable-sandbox` and Xcode receives the invocation-local `-IDEPackageSupportDisableManifestSandbox=YES` to avoid nested sandbox initialization. The outer network boundary remains active; a blocked outbound connection is probed before checks. No system defaults or signature checks change.

Only the minimal toolchain environment, user identity for XcodeGen and configured compiler wrappers are preserved; API credentials are not forwarded and remote Rust builds remain disabled. Existing Swift/Xcode caches may be reused; the no-FFI build has fresh scratch storage. This is incremental qualification, not a cold-build claim. The additional macOS C ABI check executes real Python, NumPy, PNG generation and subprocess through the existing library.

## Linux

Use an authorized Linux build environment with installed Rust, all locked sources and the exact clean SDK sibling at `../nanocodex`. Swift's wrapper graph is larger than the core workspace graph: the core's five registry crates alone are insufficient. Platform libraries and compilers remain external prerequisites.

```sh
scripts/reference-swift.sh rust 2
```

This mode enters a user/network namespace and runs `cargo check --locked --offline` with compiler wrappers preserved and `SHIPPY_REMOTE_BUILD=0`. It does not install toolchains, acquire crates or build release FFI libraries. Missing local dependencies fail closed. Do not run this potentially heavy gate on the hosted Hand.

## Combined gate

Copy each whole evidence directory, including logs, to the review machine:

```sh
scripts/reference-swift.sh gate /copied/apple/receipt.json /copied/linux/receipt.json
```

Log names are relative to each receipt, so bundles move between hosts unchanged. The gate checks platform modes, every expected successful command, exact log hashes and the current source set and hashes. Untracked compiler inputs are rejected. Source edits or new untracked source invalidate old receipts. Retain the original FFI runtime qualification separately; this runner does not replace publisher attestations or physical-device testing.
