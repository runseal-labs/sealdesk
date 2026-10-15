#!/usr/bin/env bash
set -euo pipefail

repository=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
flutter_executable=${FLUTTER:-flutter}
test_directory=$(mktemp -d -t taildesk-flutter-tests.XXXXXX)
trap 'rm -rf -- "$test_directory"' EXIT

cp "$repository/tests/taildesk_flutter/pubspec.yaml" "$test_directory/pubspec.yaml"
mkdir -p "$test_directory/lib/common" "$test_directory/lib/desktop/pages" "$test_directory/test"
mkdir -p "$test_directory/lib/mobile/pages"
ln -s "$repository/flutter/lib/mobile/pages/taildesk_scan_page.dart" "$test_directory/lib/mobile/pages/taildesk_scan_page.dart"
for source in taildesk_pairing.dart taildesk_qr_image.dart taildesk_qr_file.dart taildesk_startup.dart taildesk_connection.dart; do
  if [[ -f "$repository/flutter/lib/common/$source" ]]; then
    ln -s "$repository/flutter/lib/common/$source" "$test_directory/lib/common/$source"
  fi
done
ln -s "$repository/flutter/lib/desktop/pages/taildesk_home_page.dart" "$test_directory/lib/desktop/pages/taildesk_home_page.dart"
ln -s "$repository/flutter/test/fixtures" "$test_directory/test/fixtures"
for test_file in "$repository"/flutter/test/taildesk_*_test.dart; do
  ln -s "$test_file" "$test_directory/test/$(basename -- "$test_file")"
done
cd "$test_directory"
"$flutter_executable" --suppress-analytics pub get
"$flutter_executable" --suppress-analytics analyze --no-pub
"$flutter_executable" --suppress-analytics test --no-pub --reporter expanded --timeout 60s
