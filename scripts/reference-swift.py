import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import socket
import subprocess
import sys
import tempfile

root = Path(__file__).resolve().parent.parent
os.chdir(root)

def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()

def require(condition, message):
    if not condition:
        raise SystemExit(message)

def inputs():
    untracked = subprocess.check_output(['git', 'ls-files', '--others', '--exclude-standard', '--', 'Sources', 'src', 'include', 'Examples'], text=True).strip()
    require(not untracked, 'untracked compiler inputs must be committed before qualification: ' + untracked)
    names = subprocess.check_output(['git', 'ls-files', '-z', '--', 'Sources', 'src', 'include', 'Examples', 'Cargo.toml', 'Cargo.lock', 'Package.swift', 'sdk-revision.json', 'ts-revision.json', 'rust-toolchain.toml', '.cargo/config.toml'], text=True).split('\0')
    paths = {root / name for name in names if name}
    paths.update((root / 'scripts').glob('reference*'))
    paths.add(root / 'scripts/publish-ffi.sh')
    return {str(p.relative_to(root)): digest(p) for p in sorted(paths) if p.is_file()}

def boundary():
    require(os.environ.get('NANOCODE_REFERENCE_NETWORK') == 'denied', 'use scripts/reference-swift.sh apple|rust')
    try:
        with socket.create_connection(('1.1.1.1', 443), timeout=1):
            raise SystemExit('OS network boundary inactive')
    except OSError:
        pass

def run(work, name, command, extra=None):
    environment = dict(os.environ)
    if extra:
        environment.update(extra)
    log = work / (name + '.log')
    print('START ' + name, flush=True)
    with log.open('w') as stream:
        result = subprocess.run(command, stdout=stream, stderr=subprocess.STDOUT, env=environment)
    require(result.returncode == 0, f'{name} failed ({result.returncode}); see {log}')
    print('PASS ' + name, flush=True)
    return {'command': command, 'exit_code': result.returncode, 'log': log.name, 'log_sha256': digest(log)}

def isolated(mode, arguments):
    environment = {name: os.environ[name] for name in ('HOME', 'PATH', 'USER', 'LOGNAME', 'TMPDIR', 'DEVELOPER_DIR', 'SHIPPY_REMOTE', 'RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER', 'CARGO_BUILD_RUSTC_WRAPPER', 'CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER', 'RUSTUP_HOME', 'CARGO_HOME', 'CARGO_TARGET_DIR') if name in os.environ}
    for name in ('RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER', 'CARGO_BUILD_RUSTC_WRAPPER', 'CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER'):
        require(name not in environment or bool(environment[name]), 'empty wrapper override: ' + name)
    environment.update(SHIPPY_REMOTE_BUILD='0', CARGO_NET_OFFLINE='true', NANOCODE_REFERENCE_NETWORK='denied', LC_ALL='C')
    command = [sys.executable, '-I', str(Path(__file__).resolve()), '_' + mode, *arguments]
    if mode == 'apple':
        require(platform.system() == 'Darwin', 'apple requires macOS, Xcode 26 and XcodeGen 2.46.0')
        profile = '(version 1)(allow default)(deny network-outbound (remote ip "*:*"))(deny network-inbound (local ip "*:*"))'
        command = ['/usr/bin/sandbox-exec', '-p', profile, *command]
    else:
        require(platform.system() == 'Linux', 'rust gate must execute on Linux')
        command = ['unshare', '--user', '--map-root-user', '--net', '--', *command]
    return subprocess.call(command, env=environment)

args = sys.argv[1:]
require(bool(args), 'usage: scripts/reference-swift.sh apple ARTIFACT_DIRECTORY QUALIFICATION_JSON JOBS | rust JOBS | gate APPLE_RECEIPT RUST_RECEIPT')
mode, *arguments = args
if mode in ('apple', 'rust'):
    raise SystemExit(isolated(mode, arguments))
if mode == 'gate':
    require(len(arguments) == 2, 'gate requires Apple and Linux Rust receipts')
    receipts = [json.loads(Path(p).read_text()) for p in arguments]
    require([p.get('mode') for p in receipts] == ['apple', 'rust'], 'wrong receipt modes')
    require(all(p.get('pass') is True and p.get('source_hashes') == inputs() for p in receipts), 'failed or stale source receipt')
    require(receipts[0]['host']['system'] == 'Darwin' and receipts[1]['host']['system'] == 'Linux', 'wrong gate platforms')
    require(set(receipts[0]['checks']) == {'artifact', 'swift', 'without_ffi', 'console_build', 'console', 'c', 'python', 'macos_runtime', 'xcodegen', 'demo_macos', 'demo_ios'}, 'missing Apple gate')
    require(set(receipts[1]['checks']) == {'cargo'}, 'missing Linux Cargo gate')
    for receipt, location in zip(receipts, arguments):
        require(all(check['exit_code'] == 0 for check in receipt['checks'].values()), 'failed check')
        for check in receipt['checks'].values():
            require(Path(check['log']).name == check['log'], 'unsafe log path')
            require(digest(Path(location).resolve().parent / check['log']) == check['log_sha256'], 'changed gate log')
    print(json.dumps({'pass': True, 'gates': ['package', 'demo_macos', 'demo_ios', 'rust_linux'], 'scope': 'workflow checks only; not a publisher qualification or new iOS runtime attestation'}, indent=2))
    raise SystemExit(0)
require(mode in ('_apple', '_rust'), 'unknown mode')
boundary()
(root / 'output').mkdir(exist_ok=True)
work = Path(tempfile.mkdtemp(prefix='reference-swift-', dir=root / 'output'))
before = inputs()
checks = {}
if mode == '_apple':
    require(len(arguments) == 3, 'apple requires ARTIFACT_DIRECTORY QUALIFICATION_JSON JOBS')
    artifacts, qualification, jobs = arguments
    require(jobs.isdecimal() and int(jobs) > 0, 'JOBS must be positive')
    require(os.environ.get('NANOCODE_WITHOUT_FFI') is None, 'clear NANOCODE_WITHOUT_FFI for the linked build')
    framework = root / 'build/NanocodeFFI.xcframework'
    require(framework.is_dir(), 'stage the qualified local XCFramework in build/ first; this runner never builds or downloads FFI')
    checks['artifact'] = run(work, 'artifact', ['bash', 'scripts/publish-ffi.sh', '--verify', str(Path(artifacts).resolve()), str(Path(qualification).resolve())])
    proof = json.loads(Path(qualification).read_text())
    for identifier, library in proof['libraries'].items():
        local = framework / identifier / 'libnanocode_swift.a'
        require(digest(local) == library['sha256'], 'local XCFramework library differs: ' + identifier)
        for header in ('nanocode.h', 'module.modulemap'):
            require((framework / identifier / 'Headers' / header).read_bytes() == (root / 'include' / header).read_bytes(), 'local header mismatch')
    mac = [identifier for identifier, library in proof['libraries'].items() if library['platform'] == 'macos']
    require(len(mac) == 1, 'missing macOS library')
    library = root / 'build/macos/libnanocode_swift.a'
    library.parent.mkdir(exist_ok=True)
    if library.exists():
        require(digest(library) == proof['libraries'][mac[0]]['sha256'], 'existing macOS linked library differs; do not overwrite')
    else:
        shutil.copyfile(framework / mac[0] / 'libnanocode_swift.a', library)
    checks['swift'] = run(work, 'swift', ['swift', 'build', '--disable-sandbox', '-j', jobs, '--skip-update'])
    without = work / 'without-ffi'
    checks['without_ffi'] = run(work, 'without-ffi', ['swift', 'build', '--disable-sandbox', '-j', jobs, '--skip-update', '--scratch-path', str(without)], {'NANOCODE_WITHOUT_FFI': '1'})
    require(not list(without.rglob('*.xcframework')), 'without-FFI build contains XCFramework')
    checks['console_build'] = run(work, 'console-build', ['swift', 'build', '--disable-sandbox', '--package-path', 'Examples/Console', '-j', jobs, '--skip-update'])
    console = subprocess.check_output(['swift', 'build', '--disable-sandbox', '--package-path', 'Examples/Console', '--show-bin-path'], text=True).strip() + '/Console'
    script = 'printf "b\\na\\n" | sort | tr "\\n" " "; echo "{\\"n\\": 2}" | jq .n; printf abc | sha256sum'
    checks['console'] = run(work, 'console', [console, 'bash', script])
    require((work / 'console.log').read_text().strip() == 'a b 2\nba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  -', 'unexpected Console output')
    sdk = subprocess.check_output(['xcrun', '--sdk', 'macosx', '--show-sdk-path'], text=True).strip()
    checks['c'] = run(work, 'c', ['bash', 'Examples/C/build.sh'], {'SDKROOT': sdk})
    checks['python'] = run(work, 'python', ['bash', 'Examples/Python/build.sh'], {'SDKROOT': sdk})
    checks['macos_runtime'] = run(work, 'macos-runtime', [sys.executable, '-I', 'scripts/reference-ffi-shell.py', str(root / 'build/python/libnanocode.dylib'), str(root / 'Sources/Nanocode/Resources/shell'), str(work)])
    require(subprocess.check_output(['xcodegen', '--version'], text=True).strip() == 'Version: 2.46.0', 'XcodeGen must be 2.46.0')
    checks['xcodegen'] = run(work, 'xcodegen', ['xcodegen', 'generate', '--spec', 'Examples/Demo/project.yml'])
    for name, scheme, destination in [('demo_macos', 'NanocodeDemo-macOS', 'generic/platform=macOS'), ('demo_ios', 'NanocodeDemo-iOS', 'generic/platform=iOS Simulator')]:
        checks[name] = run(work, name, ['xcodebuild', 'build', '-project', 'Examples/Demo/NanocodeDemo.xcodeproj', '-scheme', scheme, '-destination', destination, '-derivedDataPath', 'output/DerivedData', 'ARCHS=arm64', 'CODE_SIGNING_ALLOWED=NO', '-jobs', jobs, '-disableAutomaticPackageResolution', '-skipPackageUpdates', '-IDEPackageSupportDisableManifestSandbox=YES'])
else:
    require(len(arguments) == 1 and arguments[0].isdecimal() and int(arguments[0]) > 0, 'rust requires positive JOBS')
    require(platform.system() == 'Linux', 'Linux Rust gate cannot be replaced by macOS cargo check')
    core = root.parent / 'nanocodex'
    require(subprocess.check_output(['git', '-C', str(core), 'rev-parse', 'HEAD'], text=True).strip() == json.loads((root / 'sdk-revision.json').read_text())['revision'], 'wrong sibling SDK revision')
    require(not subprocess.check_output(['git', '-C', str(core), 'status', '--porcelain', '--untracked-files=no'], text=True).strip(), 'dirty SDK sibling')
    checks['cargo'] = run(work, 'cargo', ['cargo', 'check', '--locked', '--offline', '-j', arguments[0]])
require(inputs() == before, 'qualification changed source inputs')
receipt = {'pass': True, 'mode': mode[1:], 'host': {'system': platform.system(), 'machine': platform.machine()}, 'network': 'OS-denied IP networking; local build IPC allowed', 'source_hashes': before, 'checks': checks, 'scope': 'local workflow replacement checks; not publisher qualification or FFI source rebuild'}
(work / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
print(str(work / 'receipt.json'), flush=True)
