#!/usr/bin/env bash
# Renders a standalone HTML file to the ui-capture contract's outputs with an installed
# Chromium-family browser in headless mode: <out-dir>/<state>.png at a fixed size and scale 1,
# and the state's key merged into <out-dir>/geometry.json from geometry-probe.js.
# Exit 0 on success, 1 on a render or probe failure, 2 on bad usage or when no browser is found.
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
PROBE="$SCRIPT_DIR/geometry-probe.js"
DEFAULT_SIZE=1280x800
# Lets fonts, timers and animation frames settle before the shot and the probe.
VIRTUAL_TIME_MS=3000

usage() {
  cat <<'EOF'
Usage: render.sh [--size WxH] <html> <out-dir> <state>
       render.sh --self-test

Renders <html> at WxH (default 1280x800) and scale 1 to <out-dir>/<state>.png, and writes the
state's layout violations into <out-dir>/geometry.json, creating it when absent and keeping the
other states' keys.

The browser is $UI_CAPTURE_BROWSER, else chrome, chromium or msedge on PATH, else a standard
Windows or macOS install, else the newest Chromium in the Playwright cache that starts.

--self-test renders a clean page and a page with a planted overflow, and fails unless both PNGs
exist, the clean state is empty and the overflow is reported.
EOF
}

die() {
  printf 'render.sh: %s\n' "$1" >&2
  exit "${2:-1}"
}

to_unix() {
  if command -v cygpath >/dev/null 2>&1; then cygpath -u "$1"; else printf '%s' "$1"; fi
}

# The browser on Windows is a native program, so paths handed to it use drive-letter form.
to_native() {
  if command -v cygpath >/dev/null 2>&1; then cygpath -m "$1"; else printf '%s' "$1"; fi
}

abs_path() {
  local dir
  dir=$(cd "$(dirname "$1")" && pwd) || return 1
  printf '%s/%s' "$dir" "$(basename "$1")"
}

file_url() {
  local native
  native=$(to_native "$1")
  native=$(printf '%s' "$native" | sed -e 's/%/%25/g' -e 's/ /%20/g' -e 's/#/%23/g' -e 's/?/%3F/g')
  case "$native" in
    /*) printf 'file://%s' "$native" ;;
    *) printf 'file:///%s' "$native" ;;
  esac
}

# A cache build can be left half-installed, so a candidate must start headless before it is
# chosen; a newer broken build then falls through to an older working one.
browser_starts() {
  local profile status
  profile=$(mktemp -d) || return 1
  run_browser "$1" 64 64 "$profile" --dump-dom about:blank >/dev/null 2>&1
  status=$?
  rm -rf "$profile"
  return "$status"
}

playwright_chromium() {
  local cache dir exe
  for cache in "${PLAYWRIGHT_BROWSERS_PATH:-}" \
    "${LOCALAPPDATA:+$(to_unix "$LOCALAPPDATA")/ms-playwright}" \
    "$HOME/Library/Caches/ms-playwright" \
    "${XDG_CACHE_HOME:-$HOME/.cache}/ms-playwright"; do
    [ -n "$cache" ] && [ -d "$cache" ] || continue
    while IFS= read -r dir; do
      [ -n "$dir" ] || continue
      for exe in "$dir"/chrome-win64/chrome.exe "$dir"/chrome-win/chrome.exe \
        "$dir"/chrome-linux64/chrome "$dir"/chrome-linux/chrome \
        "$dir"/chrome-mac*/Chromium.app/Contents/MacOS/Chromium \
        "$dir"/chrome-mac*/"Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing"; do
        if [ -x "$exe" ] && browser_starts "$exe"; then
          printf '%s' "$exe"
          return 0
        fi
      done
    done < <(find "$cache" -mindepth 1 -maxdepth 1 -type d -name 'chromium-[0-9]*' 2>/dev/null |
      awk -F- '{ print $NF "\t" $0 }' | sort -rn | cut -f2-)
  done
  return 1
}

find_browser() {
  local name candidate
  if [ -n "${UI_CAPTURE_BROWSER:-}" ]; then
    if command -v "$UI_CAPTURE_BROWSER" >/dev/null 2>&1; then
      command -v "$UI_CAPTURE_BROWSER"
      return 0
    fi
    candidate=$(to_unix "$UI_CAPTURE_BROWSER")
    [ -x "$candidate" ] || die "UI_CAPTURE_BROWSER is set to '$UI_CAPTURE_BROWSER', which is not an executable browser" 2
    printf '%s' "$candidate"
    return 0
  fi
  for name in chrome google-chrome google-chrome-stable chromium chromium-browser msedge microsoft-edge; do
    if command -v "$name" >/dev/null 2>&1; then
      command -v "$name"
      return 0
    fi
  done
  local program_files program_files_x86 local_app
  program_files=$(to_unix "${PROGRAMFILES:-C:/Program Files}")
  program_files_x86="$program_files (x86)"
  local_app=${LOCALAPPDATA:+$(to_unix "$LOCALAPPDATA")}
  for candidate in \
    "$program_files/Google/Chrome/Application/chrome.exe" \
    "$program_files_x86/Google/Chrome/Application/chrome.exe" \
    "${local_app:+$local_app/Google/Chrome/Application/chrome.exe}" \
    "$program_files/Chromium/Application/chrome.exe" \
    "${local_app:+$local_app/Chromium/Application/chrome.exe}" \
    "$program_files_x86/Microsoft/Edge/Application/msedge.exe" \
    "$program_files/Microsoft/Edge/Application/msedge.exe" \
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" \
    "/Applications/Chromium.app/Contents/MacOS/Chromium" \
    "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge"; do
    if [ -n "$candidate" ] && [ -x "$candidate" ]; then
      printf '%s' "$candidate"
      return 0
    fi
  done
  playwright_chromium && return 0
  die "no Chromium-family browser found; install Chrome, Chromium or Edge, or set UI_CAPTURE_BROWSER" 2
}

run_browser() {
  local browser=$1 width=$2 height=$3 profile=$4
  shift 4
  local flags=(
    --headless=new --disable-gpu --hide-scrollbars --force-device-scale-factor=1
    "--window-size=$width,$height" "--user-data-dir=$(to_native "$profile")"
    --no-first-run --no-default-browser-check --disable-extensions --mute-audio
    --force-prefers-reduced-motion "--virtual-time-budget=$VIRTUAL_TIME_MS"
  )
  if [ "$(id -u 2>/dev/null || echo 1)" = 0 ]; then flags+=(--no-sandbox); fi
  if command -v timeout >/dev/null 2>&1; then
    timeout 120 "$browser" "${flags[@]}" "$@"
  else
    "$browser" "${flags[@]}" "$@"
  fi
}

# Page-side driver appended after the probe: waits for load, fonts and two frames, merges the
# state's violations into the existing report, and leaves the result on <html> for --dump-dom.
# The JSON is ASCII-only with its newlines carried as U+0001, so it survives attribute
# serialisation as one line that sed can lift and tr can restore.
driver_js() {
  cat <<'EOF'
(function () {
  var state = JSON.parse(document.getElementById('ui-capture-state').textContent);
  var source = document.getElementById('ui-capture-existing');
  var existingText = source.textContent;
  var root = document.documentElement;
  ['ui-capture-state', 'ui-capture-existing', 'ui-capture-probe', 'ui-capture-driver'].forEach(function (id) {
    var node = document.getElementById(id);
    if (node) node.remove();
  });
  function finish() {
    try {
      var merged = JSON.parse(existingText);
      if (!merged || typeof merged !== 'object' || Array.isArray(merged)) {
        throw new Error('existing geometry.json is not an object keyed by state');
      }
      merged[state] = window.uiCaptureGeometry();
      var text = JSON.stringify(merged, null, 2)
        .replace(/[\u007f-\uffff]/g, function (c) { return '\\u' + ('000' + c.charCodeAt(0).toString(16)).slice(-4); })
        .replace(/\n/g, '\u0001');
      root.setAttribute('data-ui-capture-geometry', text);
      root.setAttribute('data-ui-capture-viewport', window.innerWidth + 'x' + window.innerHeight);
    } catch (e) {
      root.setAttribute('data-ui-capture-error', String(e && e.message || e).replace(/[^\x20-\x7e]/g, ' '));
    }
  }
  // Headless Chrome may dump the DOM before a promise, a frame callback or a timer queued at
  // load has run, so the report is written at load and rewritten by each later settle point;
  // whichever ran last before the dump is the one read.
  function afterLoad() {
    finish();
    setTimeout(finish, 0);
    requestAnimationFrame(function () { requestAnimationFrame(finish); });
    if (document.fonts && document.fonts.status !== 'loaded') document.fonts.ready.then(finish);
  }
  if (document.readyState === 'complete') afterLoad();
  else window.addEventListener('load', afterLoad);
})();
EOF
}

decode_attribute() {
  sed -e 's/&quot;/"/g' -e "s/&#39;/'/g" -e 's/&lt;/</g' -e 's/&gt;/>/g' -e 's/&amp;/\&/g'
}

render() {
  local size=$1 html=$2 out=$3 state=$4
  [[ "$size" =~ ^([0-9]+)x([0-9]+)$ ]] || die "--size takes WxH, such as 1280x800; got '$size'" 2
  local width=${BASH_REMATCH[1]} height=${BASH_REMATCH[2]}
  [ -f "$html" ] || die "no HTML file at '$html'" 2
  [ -n "$state" ] || die "the state name is empty" 2
  case "$state" in */* | *\\* | .*) die "state '$state' must be a plain file name" 2 ;; esac
  [ -f "$PROBE" ] || die "geometry-probe.js is missing beside render.sh" 1

  local browser
  # find_browser names the failure itself.
  browser=$(find_browser) || exit 2

  mkdir -p "$out"
  local html_abs out_abs png geometry
  html_abs=$(abs_path "$html")
  out_abs=$(cd "$out" && pwd)
  png="$out_abs/$state.png"
  geometry="$out_abs/geometry.json"

  local work wrapper
  work=$(mktemp -d)
  # The probe copy sits beside the page so its relative URLs resolve as the page's do.
  wrapper="$(dirname "$html_abs")/.ui-capture-$$-$(basename "$html_abs")"
  # shellcheck disable=SC2064
  trap "rm -rf '$work' '$wrapper'" EXIT

  rm -f "$png"
  run_browser "$browser" "$width" "$height" "$work/shot-profile" \
    "--screenshot=$(to_native "$png")" "$(file_url "$html_abs")" >"$work/shot.log" 2>&1 || true
  if [ ! -s "$png" ]; then
    cat "$work/shot.log" >&2
    die "the browser wrote no screenshot at $png" 1
  fi

  local existing='{}'
  if [ -f "$geometry" ]; then existing=$(cat "$geometry"); fi
  {
    cat "$html_abs"
    printf '\n<script type="application/json" id="ui-capture-state">"%s"</script>\n' \
      "$(printf '%s' "$state" | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g' -e 's/</\\u003c/g')"
    printf '<script type="application/json" id="ui-capture-existing">%s</script>\n' \
      "$(printf '%s' "$existing" | sed -e 's/</\\u003c/g')"
    printf '<script id="ui-capture-probe">\n'
    cat "$PROBE"
    printf '\n</script>\n<script id="ui-capture-driver">\n'
    driver_js
    printf '</script>\n'
  } >"$wrapper"

  # The screenshot is laid out at exactly WxH, but a page loaded for --dump-dom gets the window
  # size less the browser frame, which varies by platform. Widen the window by the measured
  # shortfall until the probe sees the shot's viewport.
  local win_w=$width win_h=$height attempt error encoded viewport
  for attempt in 1 2 3; do
    run_browser "$browser" "$win_w" "$win_h" "$work/dom-profile-$attempt" \
      --dump-dom "$(file_url "$wrapper")" >"$work/dom.html" 2>"$work/dom.log" || true
    error=$(sed -n 's/.*data-ui-capture-error="\([^"]*\)".*/\1/p' "$work/dom.html" | head -n 1 | decode_attribute)
    [ -z "$error" ] || die "geometry probe failed for state '$state': $error" 1
    encoded=$(sed -n 's/.*data-ui-capture-geometry="\([^"]*\)".*/\1/p' "$work/dom.html" | head -n 1)
    if [ -z "$encoded" ]; then
      cat "$work/dom.log" >&2
      die "the geometry probe returned nothing for state '$state'" 1
    fi
    viewport=$(sed -n 's/.*data-ui-capture-viewport="\([0-9]*x[0-9]*\)".*/\1/p' "$work/dom.html" | head -n 1)
    [ "$viewport" = "${width}x${height}" ] && break
    [[ "$viewport" =~ ^([0-9]+)x([0-9]+)$ ]] || die "the geometry probe reported no viewport for state '$state'" 1
    win_w=$((win_w + width - BASH_REMATCH[1]))
    win_h=$((win_h + height - BASH_REMATCH[2]))
    viewport=
  done
  [ -n "$viewport" ] || die "could not size the probe's viewport to ${width}x${height} for state '$state'" 1
  printf '%s\n' "$encoded" | decode_attribute | tr '\001' '\n' >"$work/geometry.json"
  mv -f "$work/geometry.json" "$geometry"
  printf 'render.sh: %s -> %s, %s\n' "$state" "$png" "$geometry"
}

self_test() {
  local dir
  dir=$(mktemp -d)
  # shellcheck disable=SC2064
  trap "rm -rf '$dir'" EXIT
  cat >"$dir/clean.html" <<'EOF'
<!doctype html>
<html><head><meta charset="utf-8"><style>
body { margin: 24px; font: 16px sans-serif; }
.card { width: 320px; padding: 12px; border: 1px solid #888; }
</style></head>
<body><div class="card"><h1>Settings</h1><p>Everything here fits its box.</p></div></body></html>
EOF
  cat >"$dir/planted.html" <<'EOF'
<!doctype html>
<html><head><meta charset="utf-8"><style>
body { margin: 24px; font: 16px sans-serif; }
.card { width: 200px; padding: 12px; border: 1px solid #888; }
.label { white-space: nowrap; }
</style></head>
<body><div class="card"><p class="label" id="planted-label">A label far too long for the card it sits in</p></div></body></html>
EOF
  local status=0
  (render 800x600 "$dir/clean.html" "$dir/out" clean) || status=$?
  [ "$status" -eq 0 ] || { echo "self-test: rendering the clean page failed (exit $status)" >&2; exit "$status"; }
  (render 800x600 "$dir/planted.html" "$dir/out" planted) || status=$?
  [ "$status" -eq 0 ] || { echo "self-test: rendering the planted page failed (exit $status)" >&2; exit "$status"; }

  local failed=0
  [ -s "$dir/out/clean.png" ] || { echo "self-test: clean.png missing" >&2; failed=1; }
  [ -s "$dir/out/planted.png" ] || { echo "self-test: planted.png missing" >&2; failed=1; }
  grep -q '"clean": \[\]' "$dir/out/geometry.json" ||
    { echo "self-test: the clean state is not an empty array" >&2; failed=1; }
  grep -q '"planted": \[' "$dir/out/geometry.json" ||
    { echo "self-test: the planted state has no key" >&2; failed=1; }
  grep -q '"kind": "overflow"' "$dir/out/geometry.json" ||
    { echo "self-test: the planted overflow was not reported" >&2; failed=1; }
  grep -q 'planted-label' "$dir/out/geometry.json" ||
    { echo "self-test: the overflow does not name the planted label" >&2; failed=1; }
  if [ "$failed" -ne 0 ]; then
    cat "$dir/out/geometry.json" >&2 || true
    exit 1
  fi
  echo "self-test: pass"
}

size=$DEFAULT_SIZE
args=()
while [ $# -gt 0 ]; do
  case "$1" in
    --self-test) self_test; exit 0 ;;
    --size) [ $# -ge 2 ] || die "--size needs WxH" 2; size=$2; shift 2 ;;
    --size=*) size=${1#--size=}; shift ;;
    -h | --help) usage; exit 0 ;;
    --) shift; args+=("$@"); break ;;
    -*) usage >&2; die "unknown option '$1'" 2 ;;
    *) args+=("$1"); shift ;;
  esac
done
[ ${#args[@]} -eq 3 ] || { usage >&2; exit 2; }
render "$size" "${args[0]}" "${args[1]}" "${args[2]}"
