#!/usr/bin/env bash
set -euo pipefail

MAV_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
source "$MAV_ROOT/tools/platform/lib.sh"

require_command xcodegen
require_command jq

bash "$MAV_ROOT/tools/platform/build_ios.sh"
SIMULATOR_ID="$({
  xcrun simctl list devices available -j |
    jq -r '.devices | to_entries[] | .value[] | select(.isAvailable and (.name | startswith("iPhone"))) | .udid' |
    head -n 1
} || true)"
[[ -n "$SIMULATOR_ID" && "$SIMULATOR_ID" != "null" ]] ||
  die "no available iPhone simulator found; install an iOS simulator runtime in Xcode"
(
  cd "$MAV_ROOT/apps/ios"
  xcodegen generate --spec project.yml
  xcodebuild \
    -project Maverick.xcodeproj \
    -scheme Maverick \
    -sdk iphonesimulator \
    -destination "platform=iOS Simulator,id=$SIMULATOR_ID" \
    test
)
