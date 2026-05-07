#!/bin/bash
set -e

VERSION="1.0.0"
APP="Claude Meter.app"
COMPONENT_PKG="claude-meter-component.pkg"
FINAL_PKG="Claude Meter Installer.pkg"
DIST_XML="_dist.xml"
RESOURCES_DIR="_resources"

echo "==> Building release binary..."
cargo build --release

echo "==> Building app bundle..."
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
    <string>1.0.0</string>
    <key>CFBundleShortVersionString</key>
    <string>1.0.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>LSMinimumSystemVersion</key>
    <string>12.0</string>
    <key>LSUIElement</key>
    <false/>
</dict>
</plist>
EOF

echo "==> Creating component package..."
pkgbuild \
    --component "$APP" \
    --install-location "/Applications" \
    --identifier "com.local.claude-meter" \
    --version "$VERSION" \
    "$COMPONENT_PKG"

echo "==> Creating distribution XML..."
cat > "$DIST_XML" << EOF
<?xml version="1.0" encoding="utf-8"?>
<installer-gui-script minSpecVersion="1">
    <title>Claude Meter</title>
    <organization>com.local</organization>
    <domains enable_localSystem="true"/>
    <options customize="never" require-scripts="false" rootVolumeOnly="true"/>
    <choices-outline>
        <line choice="default">
            <line choice="com.local.claude-meter"/>
        </line>
    </choices-outline>
    <choice id="default"/>
    <choice id="com.local.claude-meter" visible="false">
        <pkg-ref id="com.local.claude-meter"/>
    </choice>
    <pkg-ref id="com.local.claude-meter" version="$VERSION" onConclusion="none">$COMPONENT_PKG</pkg-ref>
</installer-gui-script>
EOF

echo "==> Creating installer package..."
mkdir -p "$RESOURCES_DIR"
productbuild \
    --distribution "$DIST_XML" \
    --resources "$RESOURCES_DIR" \
    --package-path "." \
    "$FINAL_PKG"

echo "==> Cleaning up intermediates..."
rm -f "$COMPONENT_PKG" "$DIST_XML"
rm -rf "$RESOURCES_DIR"

echo ""
echo "Done: $FINAL_PKG"
