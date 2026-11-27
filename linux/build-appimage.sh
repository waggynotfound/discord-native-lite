#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"
root=$PWD
target=x86_64-unknown-linux-musl
version=$(sed -n 's/^version = "\([^"]*\)"$/\1/p' Cargo.toml | head -1)
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$root/target}"
mkdir -p "$CARGO_TARGET_DIR" out
stage=$(mktemp -d "$CARGO_TARGET_DIR/package.XXXXXXXX")
trap 'rm -r -- "$stage"' EXIT
app="$stage/Relay.AppDir"
lic="$app/usr/share/doc/relay/licenses"
mkdir -p "$app/usr/bin" "$lic" "$app/usr/share/applications" "$app/usr/share/icons/hicolor/256x256/apps"
cargo build --locked --release --target "$target"
install -m755 "$CARGO_TARGET_DIR/$target/release/relay-linux" "$app/usr/bin/relay-linux"
ln -s usr/bin/relay-linux "$app/AppRun"
install -m644 relay.desktop "$app/usr/share/applications/relay.desktop"
ln -s usr/share/applications/relay.desktop "$app/relay.desktop"
install -m644 assets/relay.png "$app/usr/share/icons/hicolor/256x256/apps/relay.png"
ln -s usr/share/icons/hicolor/256x256/apps/relay.png "$app/relay.png"
ln -s relay.png "$app/.DirIcon"
install -m644 ../LICENSE README.md "$app/usr/share/doc/relay/"
install -m644 assets/OFL.txt assets/APPIMAGE-RUNTIME-LICENSE.txt "$lic/"
cp "$(rustc --print sysroot)/share/doc/rust/COPYRIGHT-library.html" "$lic/rust-library.html"
cargo metadata --locked --format-version 1 --filter-platform "$target" > "$stage/crates.json"
python3 - "$stage/crates.json" "$lic/crate-notices.txt" <<'PY'
import json,pathlib,re,sys
meta=json.loads(pathlib.Path(sys.argv[1]).read_text()); ids={n['id'] for n in meta['resolve']['nodes']}
with open(sys.argv[2],'w',encoding='utf-8') as out:
 for c in sorted(meta['packages'],key=lambda x:(x['name'],x['version'])):
  if not c['source'] or c['id'] not in ids: continue
  files=sorted(p for p in pathlib.Path(c['manifest_path']).parent.iterdir() if p.is_file() and re.match('^(licen[sc]e|copying|notice|unlicense|copyright)',p.name,re.I))
  if not files: sys.exit('No license text: '+c['name'])
  print(c['name'],c['version'],c['license'],file=out)
  for p in files: print(p.name,p.read_text(encoding='utf-8'),sep='\n',file=out)
PY
# Use exact upstream artifacts; never silently accept a changed continuous build.
fetch() {
  local dest=$1 url=$2 hash=$3
  if [[ ! -f "$dest" ]]; then curl --retry 2 --connect-timeout 15 --max-time 120 -fsSL -H 'Accept: application/octet-stream' "$url" -o "$dest"; fi
  printf '%s  %s\n' "$hash" "$dest" | sha256sum -c -
}
tool=${APPIMAGETOOL:-$stage/tool}; runtime=${RELAY_RUNTIME:-$stage/runtime}
fetch "$tool" https://api.github.com/repos/AppImage/appimagetool/releases/assets/715388534 e8c1169c4af66c3184d7ee110713d4be04befe671fa358e2c1591ee840553b03
fetch "$runtime" https://api.github.com/repos/AppImage/type2-runtime/releases/assets/657374641 617aa77314d57c8b8e4fae9c3f0b35af7b30d5529cb69e98f64f92510ed98a46
chmod +x "$tool"
name="Relay-$version-linux-x86_64.AppImage"
export ARCH=x86_64 VERSION="$version"
"$tool" --appimage-extract-and-run --no-appstream --comp zstd --runtime-file "$runtime" "$app" "$stage/$name"
install -m755 "$stage/$name" "out/$name"
( cd out; sha256sum "$name" > "$name.sha256"; sha256sum -c "$name.sha256" )
