#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
image=$(realpath "${1:-out/Relay-2.1.0-linux-x86_64.AppImage}")
export APPIMAGE_EXTRACT_AND_RUN=1
scratch=$(mktemp -d); gui=''
cleanup(){ touch "$scratch/finish"; if [[ -n "$gui" ]]; then kill "$gui" 2>/dev/null || true; fi; sleep .4; rm -r -- "$scratch"; }
trap cleanup EXIT
# Compile with the runner's Rust installation before isolating the app's HOME.
rustc --edition=2021 tests/fixture.rs -o "$scratch/Discord"
export HOME="$scratch/home" XDG_STATE_HOME="$scratch/state"
mkdir -p "$HOME" "$scratch/resources" out/preview
printf 'Vencord fixture' > "$scratch/resources/app.asar"
cmd=("$image" --discord "$scratch/Discord")
"$image" --version
"$image" --help | grep -q 'Flatpak'
if "$image" --discord "$scratch/missing" --launch strong; then echo 'Invalid executable accepted' >&2; exit 1; fi
# Inspect the actual packaged GUI (not a rendering mock). Only this test client is targeted.
"${cmd[@]}" >"$scratch/window.log" 2>&1 & gui=$!
win=$(timeout 15 xdotool search --sync --onlyvisible --name '^Relay \| Discord launcher$' | head -1)
sleep .5; import -window "$win" out/preview/launch.png
xdotool mousemove --window "$win" 68 194 click 1; sleep .3; import -window "$win" out/preview/compare.png
xdotool windowfocus "$win" key Escape; wait "$gui"; gui=''
for profile in stock balanced strong; do
  rm -f "$scratch/finish" "$scratch/receipt"
  "${cmd[@]}" --launch "$profile"
  ok=0; for _ in {1..40}; do status=$("${cmd[@]}" --status); if grep -q 'Processes: 2' <<<"$status" && ! grep -q 'Profile: Unverified' <<<"$status"; then ok=1; break; fi; sleep .1; done
  [[ $ok == 1 ]]; echo "$status"; grep -Eq 'Private RAM: [0-9]+ MiB' <<<"$status"
  case "$profile" in
    stock) [[ ! -s "$scratch/receipt" ]]; grep -q 'Profile: Stock' <<<"$status";;
    balanced) [[ $(cat "$scratch/receipt") == '--js-flags=--optimize-for-size' ]]; grep -q 'Profile: Balanced' <<<"$status";;
    strong) [[ $(cat "$scratch/receipt") == '--js-flags=--optimize-for-size --memory-saver-mode --heap-growing-percent=15' ]]; grep -q 'Profile: Strong saver' <<<"$status";;
  esac
  if "${cmd[@]}" --launch stock; then echo 'Duplicate launch not blocked' >&2; exit 1; fi
  if [[ "$profile" == strong ]]; then "${cmd[@]}" --record; compgen -G "$XDG_STATE_HOME/relay/readings/Stable-*-strong.tsv" >/dev/null; fi
  touch "$scratch/finish"; for _ in {1..40}; do if "${cmd[@]}" --status | grep -q 'Processes: 0'; then break; fi; sleep .1; done
done
echo 'Packaged desktop views, all profile arguments, duplicate guard and recording passed.'
