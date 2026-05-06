#!/bin/bash
set -e

cargo build --release

APP="Claude Meter.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS"

cp target/release/claude_meter "$APP/Contents/MacOS/claude_meter"

cat > "$APP/Contents/Info.plist" << 'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>Claude Meter</string>
    <key>CFBundleDisplayName</key>
    <string>Claude Meter</string>
    <key>CFBundleExecutable</key>
    <string>claude_meter</string>
    <key>CFBundleIdentifier</key>
    <string>com.local.claude-meter</string>
    <key>CFBundleVersion</key>
    <string>1.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>LSMinimumSystemVersion</key>
    <string>12.0</string>
</dict>
</plist>
EOF

echo "Built: $APP"
echo "Install: cp -r \"$APP\" ~/Applications/"
