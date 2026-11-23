#!/usr/bin/env bash
# Starts the debug build with remote debugging, runs the layout check, then stops only the app it started.
cd "$(dirname "$0")/.." || exit 1
PORT=9352
EXE="target/debug/midi-app.exe"
if tasklist //FI "IMAGENAME eq midi-app.exe" //NH | grep -q midi-app; then
  echo "another midi-app is running; not starting a check"
  exit 2
fi
PID=$(powershell -NoProfile -Command "\$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS='--remote-debugging-port=$PORT'; (Start-Process -FilePath '$EXE' -PassThru).Id")
node tools/check-ui-layout.mjs "$PORT"
RESULT=$?
taskkill //F //PID "$PID" >/dev/null 2>&1
exit $RESULT
