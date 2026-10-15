#!/usr/bin/env bash
set -euo pipefail

repository=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
flutter_executable=${FLUTTER:-flutter}
test_directory=$(mktemp -d -t sealdesk-flutter-tests.XXXXXX)
trap 'rm -rf -- "$test_directory"' EXIT

cp "$repository/tests/sealdesk_flutter/pubspec.yaml" "$test_directory/pubspec.yaml"
mkdir -p "$test_directory/lib/common" "$test_directory/lib/desktop/pages" "$test_directory/test"
mkdir -p "$test_directory/lib/mobile/pages"
ln -s "$repository/flutter/lib/mobile/pages/sealdesk_scan_page.dart" "$test_directory/lib/mobile/pages/sealdesk_scan_page.dart"
for source in sealdesk_pairing.dart sealdesk_qr_image.dart sealdesk_qr_file.dart sealdesk_startup.dart sealdesk_connection.dart; do
  if [[ -f "$repository/flutter/lib/common/$source" ]]; then
    ln -s "$repository/flutter/lib/common/$source" "$test_directory/lib/common/$source"
  fi
done
ln -s "$repository/flutter/lib/desktop/pages/sealdesk_home_page.dart" "$test_directory/lib/desktop/pages/sealdesk_home_page.dart"
ln -s "$repository/flutter/test/fixtures" "$test_directory/test/fixtures"
for test_file in "$repository"/flutter/test/sealdesk_*_test.dart; do
  ln -s "$test_file" "$test_directory/test/$(basename -- "$test_file")"
done
cd "$test_directory"
"$flutter_executable" --suppress-analytics pub get
"$flutter_executable" --suppress-analytics analyze --no-pub
"$flutter_executable" --suppress-analytics test --no-pub --reporter expanded --timeout 60s
