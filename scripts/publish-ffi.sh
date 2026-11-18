#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
repository=nanocodexyz/nanocode-swift
fingerprint=$(cat Cargo.toml Cargo.lock sdk-revision.json rust-toolchain.toml scripts/build-ffi.sh include/* $(find src -type f | sort) | shasum -a 256 | cut -c1-20)
if [[ "${1:-}" == --fingerprint && $# == 1 ]]; then
  echo "$fingerprint"
  exit 0
fi
if [[ $# != 3 || ( "$1" != --verify && "$1" != --upload ) ]]; then
  echo 'usage: scripts/publish-ffi.sh --verify|--upload ARTIFACT_DIRECTORY QUALIFICATION_JSON' >&2
  exit 2
fi
mode=$1
artifacts=$(cd "$2" && pwd)
qualification=$(cd "$(dirname "$3")" && pwd)/$(basename "$3")
tag="ffi-$fingerprint"
url="https://github.com/$repository/releases/download/$tag/NanocodeFFI.xcframework.zip"
mkdir -p output
work=$(mktemp -d "$PWD/output/ffi-publish.XXXXXX")
mkdir "$work/staged"
cp "$artifacts/NanocodeFFI.xcframework.zip" "$work/staged/NanocodeFFI.xcframework.zip"
cp "$artifacts/ffi-release.json" "$work/staged/ffi-release.json"
cp "$qualification" "$work/staged/qualification.json"
artifacts="$work/staged"
qualification="$artifacts/qualification.json"
python3 -I - "$fingerprint" "$url" "$artifacts" "$qualification" "$work" <<'PY'
import hashlib,json,pathlib,plistlib,shutil,stat,subprocess,sys,zipfile
fingerprint,url,artifacts,qualification,work=sys.argv[1:]
artifacts=pathlib.Path(artifacts);work=pathlib.Path(work)
manifest=json.loads((artifacts/'ffi-release.json').read_text());proof=json.loads(pathlib.Path(qualification).read_text());archive=artifacts/'NanocodeFFI.xcframework.zip'
def digest(path):
 with path.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
assert set(manifest)=={'url','checksum','fingerprint'},'manifest fields'
assert manifest['url']==url and manifest['fingerprint']==fingerprint,'manifest identity'
assert digest(archive)==manifest['checksum']==proof['checksum'],'archive checksum'
assert proof['fingerprint']==fingerprint,'qualification fingerprint'
assert proof['core']==json.loads(pathlib.Path('sdk-revision.json').read_text())['revision'],'qualified SDK'
assert proof['ts']==json.loads(pathlib.Path('ts-revision.json').read_text())['revision'],'qualified shell'
assert subprocess.check_output(['git','-C','../nanocodex','rev-parse','HEAD'],text=True).strip()==proof['core'],'SDK checkout revision'
assert not subprocess.check_output(['git','-C','../nanocodex','status','--porcelain','--untracked-files=no'],text=True).strip(),'dirty SDK checkout'
checks={'swift','without_ffi','console','offline_shell','c','python','demo_macos','demo_ios','ios_runtime','macos_runtime','xcframework'}
assert set(proof['checks'])==checks and all(v is True for v in proof['checks'].values()),'incomplete qualification'
inputs={pathlib.Path(p) for p in ['Cargo.toml','Cargo.lock','sdk-revision.json','ts-revision.json','rust-toolchain.toml','scripts/build-ffi.sh','.cargo/config.toml','Package.swift']}
for directory in ['include','src','Sources']:
 inputs.update(p for p in pathlib.Path(directory).rglob('*') if p.is_file())
assert set(proof['source_hashes'])=={str(p) for p in inputs},'qualification input set'
for p in inputs:assert digest(p)==proof['source_hashes'][str(p)],f'changed input: {p}'
allowed={'NanocodeFFI.xcframework/Info.plist'}
expected={('macos',None,('arm64','x86_64')),('ios',None,('arm64',)),('ios','simulator',('arm64',))};seen=set();libraries={}
with zipfile.ZipFile(archive) as z:
 names=z.namelist();assert len(names)==len(set(names)),'duplicate archive member'
 for info in z.infolist():
  p=pathlib.PurePosixPath(info.filename)
  assert not p.is_absolute() and '..' not in p.parts and p.parts[0] in {'NanocodeFFI.xcframework','__MACOSX'},'archive path'
  assert not stat.S_ISLNK(info.external_attr>>16),'archive symlink'
 entries=plistlib.loads(z.read('NanocodeFFI.xcframework/Info.plist'))['AvailableLibraries'];assert len(entries)==3,'platform count'
 for entry in entries:
  key=(entry['SupportedPlatform'],entry.get('SupportedPlatformVariant'),tuple(sorted(entry['SupportedArchitectures'])))
  assert key in expected and key not in seen,'platform mapping';seen.add(key)
  identifier=entry['LibraryIdentifier'];assert pathlib.PurePosixPath(identifier).name==identifier and identifier not in {'.','..'},'library identifier'
  assert entry['LibraryPath']=='libnanocode_swift.a' and entry['HeadersPath']=='Headers','library layout'
  member=f"NanocodeFFI.xcframework/{identifier}/{entry['LibraryPath']}";assert member in names,'library member';allowed.add(member)
  for header in ['nanocode.h','module.modulemap']:
   name=f'NanocodeFFI.xcframework/{identifier}/Headers/{header}';assert z.read(name)==(pathlib.Path('include')/header).read_bytes(),'header bytes';allowed.add(name)
  destination=work/(identifier+'.a')
  with z.open(member) as source,destination.open('wb') as target:shutil.copyfileobj(source,target)
  arch=tuple(sorted(subprocess.check_output(['lipo','-archs',str(destination)],text=True).split()));assert arch==key[2],'library architectures'
  library=proof['libraries'][identifier]
  assert (library['platform'],library.get('variant'),tuple(sorted(library['architectures'])))==key,'qualified platform'
  assert digest(destination)==library['sha256'] and destination.stat().st_size==library['bytes'],'qualified library bytes'
  libraries[identifier]=library
 assert all(i.is_dir() or i.filename.startswith('__MACOSX/') or i.filename in allowed for i in z.infolist()),'unexpected framework file'
assert seen==expected and set(libraries)==set(proof['libraries']),'qualified library set'
(work/'verified.json').write_text(json.dumps({'fingerprint':fingerprint,'checksum':manifest['checksum'],'libraries':libraries,'source_hashes':proof['source_hashes']},indent=2)+'\n')
PY
echo "qualified artifact verified: $work/verified.json"
[[ "$mode" == --upload ]] || exit 0
if gh api --hostname github.com "repos/$repository/releases/tags/$tag" >"$work/release-before.json"; then
  jq -e --arg tag "$tag" '.tag_name == $tag and .draft == false and .prerelease == false' "$work/release-before.json" >/dev/null
else
  jq -e '.status == "404"' "$work/release-before.json" >/dev/null
  gh release create "$tag" "$artifacts/NanocodeFFI.xcframework.zip" "$artifacts/ffi-release.json" \
    --repo "https://github.com/$repository" --target "$(git rev-parse HEAD)" --title "NanocodeFFI $fingerprint" \
    --notes "The nanocode SDK at $(jq -r .revision sdk-revision.json) built for macOS (arm64 and x86_64), iOS and the iOS simulator (arm64). Package.swift reads ffi-release.json."
fi
curl --fail --location --silent --show-error "$url" -o "$work/downloaded.zip"
curl --fail --location --silent --show-error "${url%/*}/ffi-release.json" -o "$work/downloaded.json"
cmp "$artifacts/NanocodeFFI.xcframework.zip" "$work/downloaded.zip"
cmp "$artifacts/ffi-release.json" "$work/downloaded.json"
cp "$work/downloaded.json" ffi-release.json
echo "published assets verified: https://github.com/$repository/releases/tag/$tag"
echo "commit ffi-release.json only after a fresh consumer build; evidence: $work"
