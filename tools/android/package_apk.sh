#!/usr/bin/env bash
set -euo pipefail

SO_PATH=""
RESOURCES_DIR=""
OUTPUT_APK=""
VERSION_NAME="26.3-android"
VERSION_CODE="2"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --so) SO_PATH="$2"; shift 2 ;;
    --resources-dir) RESOURCES_DIR="$2"; shift 2 ;;
    --output) OUTPUT_APK="$2"; shift 2 ;;
    --version-name) VERSION_NAME="$2"; shift 2 ;;
    --version-code) VERSION_CODE="$2"; shift 2 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

if [[ -z "$SO_PATH" || -z "$OUTPUT_APK" ]]; then
  echo "usage: $0 --so <libhorizon_android.so> --output <out.apk> [--resources-dir <optional_local_prepared_dir>] [--version-name <str>] [--version-code <int>]" >&2
  exit 2
fi

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SDK_ROOT="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-$HOME/Library/Android/sdk}}"
ANDROID_JAR="${ANDROID_JAR:-$SDK_ROOT/platforms/android-35/android.jar}"

BUILD_TOOLS="${ANDROID_BUILD_TOOLS:-}"
if [[ -z "$BUILD_TOOLS" ]]; then
  BUILD_TOOLS="$(ls -1d "$SDK_ROOT"/build-tools/35.* "$SDK_ROOT"/build-tools/34.* 2>/dev/null | sort -V | tail -n 1)"
fi

if [[ ! -f "$ANDROID_JAR" || ! -x "$BUILD_TOOLS/aapt2" ]]; then
  echo "Android SDK platform 35 or build-tools not found under $SDK_ROOT" >&2
  exit 1
fi

JAVAC_BIN="${JAVAC:-javac}"
WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

echo "[apk] compiling Android XML resources with aapt2..."
"$BUILD_TOOLS/aapt2" compile \
  --dir "$REPO_ROOT/android-apk/res" \
  -o "$WORK_DIR/res.zip"

"$BUILD_TOOLS/aapt2" link \
  -I "$ANDROID_JAR" \
  --manifest "$REPO_ROOT/android-apk/AndroidManifest.xml" \
  --version-code "$VERSION_CODE" \
  --version-name "$VERSION_NAME" \
  -0 .zip \
  -o "$WORK_DIR/unsigned.apk" \
  "$WORK_DIR/res.zip"

echo "[apk] compiling Java Activities -> classes.dex..."
mkdir -p "$WORK_DIR/classes" "$WORK_DIR/stage/lib/arm64-v8a"
"$JAVAC_BIN" \
  -encoding UTF-8 \
  -source 8 -target 8 \
  -bootclasspath "$ANDROID_JAR" \
  -d "$WORK_DIR/classes" \
  "$REPO_ROOT/android-apk/src/dev/horizon/android/"*.java

"$BUILD_TOOLS/d8" \
  --min-api 28 \
  --lib "$ANDROID_JAR" \
  --output "$WORK_DIR/stage" \
  "$WORK_DIR/classes/dev/horizon/android/"*.class

echo "[apk] staging native library libhorizon_android.so..."
cp "$SO_PATH" "$WORK_DIR/stage/lib/arm64-v8a/libhorizon_android.so"
if [[ -n "${LLVM_STRIP:-}" && -x "${LLVM_STRIP}" ]]; then
  "$LLVM_STRIP" --strip-debug "$WORK_DIR/stage/lib/arm64-v8a/libhorizon_android.so" || true
fi

if [[ -n "$RESOURCES_DIR" ]]; then
  if [[ ! -f "$RESOURCES_DIR/horizon-resources.json" ]]; then
    echo "Prepared resources missing horizon-resources.json in $RESOURCES_DIR" >&2
    exit 1
  fi
  echo "[apk] archiving local private resources into assets/resources.zip..."
  mkdir -p "$WORK_DIR/stage/assets"
  (
    cd "$RESOURCES_DIR"
    zip -qr -6 "$WORK_DIR/stage/assets/resources.zip" . -x ".horizon-prepare.lock"
  )
fi

echo "[apk] merging DEX and native arm64 library into APK..."
(
  cd "$WORK_DIR/stage"
  zip -q -g "$WORK_DIR/unsigned.apk" classes.dex lib/arm64-v8a/libhorizon_android.so
  if [[ -f "assets/resources.zip" ]]; then
    zip -q -0 -g "$WORK_DIR/unsigned.apk" assets/resources.zip
  fi
)

echo "[apk] aligning APK (16 KB page alignment)..."
"$BUILD_TOOLS/zipalign" -P 16 -f 4 "$WORK_DIR/unsigned.apk" "$WORK_DIR/aligned.apk"

mkdir -p "$(dirname "$OUTPUT_APK")"
KEYSTORE_PATH="$REPO_ROOT/android-apk/debug.keystore"
KS_PASS="pass:android"
KEY_ALIAS="androiddebugkey"
KEY_PASS="pass:android"

if [[ -n "${ANDROID_KEYSTORE_BASE64:-}" ]]; then
  KEYSTORE_PATH="$WORK_DIR/release.keystore"
  printf '%s' "$ANDROID_KEYSTORE_BASE64" | base64 --decode > "$KEYSTORE_PATH"
  KS_PASS="pass:${ANDROID_KEYSTORE_PASS:?missing ANDROID_KEYSTORE_PASS}"
  KEY_ALIAS="${ANDROID_KEY_ALIAS:?missing ANDROID_KEY_ALIAS}"
  KEY_PASS="pass:${ANDROID_KEY_PASS:-$ANDROID_KEYSTORE_PASS}"
fi

echo "[apk] signing and verifying APK..."
"$BUILD_TOOLS/apksigner" sign \
  --v2-signing-enabled true \
  --v3-signing-enabled true \
  --ks "$KEYSTORE_PATH" \
  --ks-pass "$KS_PASS" \
  --ks-key-alias "$KEY_ALIAS" \
  --key-pass "$KEY_PASS" \
  --out "$OUTPUT_APK" \
  "$WORK_DIR/aligned.apk"

"$BUILD_TOOLS/apksigner" verify --verbose "$OUTPUT_APK"
ls -lh "$OUTPUT_APK"
