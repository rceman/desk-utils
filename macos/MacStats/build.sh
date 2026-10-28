#!/bin/bash
set -euo pipefail

SRC="$(cd "$(dirname "$0")" && pwd)/main.swift"
APP="$HOME/Applications/MacStats.app"
BIN="$APP/Contents/MacOS/MacStats"

mkdir -p "$APP/Contents/MacOS"
swiftc -O -o "$BIN" "$SRC" -framework AppKit -framework Foundation -framework IOKit

cat > "$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>MacStats</string>
    <key>CFBundleDisplayName</key>
    <string>MacStats</string>
    <key>CFBundleIdentifier</key>
    <string>com.user.macstats</string>
    <key>CFBundleExecutable</key>
    <string>MacStats</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleVersion</key>
    <string>1</string>
    <key>CFBundleShortVersionString</key>
    <string>1.0</string>
    <key>LSUIElement</key>
    <true/>
    <key>LSMinimumSystemVersion</key>
    <string>14.0</string>
</dict>
</plist>
PLIST

codesign --force --sign - "$APP"
echo "Built $APP"
