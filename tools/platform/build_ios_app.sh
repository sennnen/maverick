#!/usr/bin/env bash
set -euo pipefail

MAV_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
source "$MAV_ROOT/tools/platform/lib.sh"

require_command xcodegen

bash "$MAV_ROOT/tools/platform/build_ios.sh"
(
  cd "$MAV_ROOT/apps/ios"
  xcodegen generate --spec project.yml
  xcodebuild \
    -project Maverick.xcodeproj \
    -scheme Maverick \
    -sdk iphonesimulator \
    -destination 'platform=iOS Simulator,name=iPhone 16 Pro' \
    test
)
