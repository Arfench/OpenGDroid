#!/usr/bin/env bash
# Builds and signs the OpenGDroid APK.
#
# Requires: cargo, NDK r27c, JDK 21, build-tools (aapt2/d8/apksigner/zipalign)
# and an android.jar. Paths are read from the environment so the script works
# on any machine; see android/README.md.
set -euo pipefail

cd "$(dirname "$0")"

: "${JAVA_HOME:?set JAVA_HOME to a JDK 21 install}"
: "${ANDROID_JAR:?set ANDROID_JAR to a platform android.jar}"
: "${BUILD_TOOLS:?set BUILD_TOOLS to a build-tools directory}"

AAPT2="$BUILD_TOOLS/aapt2.exe"
ZIPALIGN="$BUILD_TOOLS/zipalign.exe"
D8_JAR="$BUILD_TOOLS/lib/d8.jar"
APKSIGNER_JAR="$BUILD_TOOLS/lib/apksigner.jar"
NDK_BIN="${NDK_BIN:?set NDK_BIN to the NDK toolchain bin directory}"

MIN_SDK=26
TARGET_SDK=32
LIBS_DIR=../target/aarch64-linux-android/release
SO="$LIBS_DIR/libopengdroid.so"

echo "==> building native library"
( cd .. && cargo build --release --target aarch64-linux-android )
test -f "$SO"

echo "==> compiling java"
rm -rf build
mkdir -p build/classes build/dex build/lib/arm64-v8a
"$JAVA_HOME/bin/javac" --release 11 -encoding UTF-8 -cp "$(cygpath -w "$ANDROID_JAR" 2>/dev/null || echo "$ANDROID_JAR")" \
    -d build/classes app/MainActivity.java Core.java

echo "==> dexing"
"$JAVA_HOME/bin/java" -cp "$(cygpath -w "$D8_JAR" 2>/dev/null || echo "$D8_JAR")" com.android.tools.r8.D8 \
    --min-api "$MIN_SDK" --output build/dex \
    $(find build/classes -name '*.class')

echo "==> linking resources"
"$AAPT2" link -o build/base.apk --manifest app/AndroidManifest.xml \
    -I "$(cygpath -w "$ANDROID_JAR" 2>/dev/null || echo "$ANDROID_JAR")" \
    --min-sdk-version "$MIN_SDK" --target-sdk-version "$TARGET_SDK" \
    --version-code 1 --version-name 0.1.0

echo "==> packaging"
cp "$SO" build/lib/arm64-v8a/
python - "$(cygpath -w "$(pwd)/build")" <<'PY'
import shutil, sys, zipfile, pathlib
build = pathlib.Path(sys.argv[1])
shutil.copyfile(build / "base.apk", build / "app-unsigned.apk")
with zipfile.ZipFile(build / "app-unsigned.apk", "a", zipfile.ZIP_DEFLATED) as z:
    z.write(build / "dex/classes.dex", "classes.dex")
    z.write(build / "lib/arm64-v8a/libopengdroid.so", "lib/arm64-v8a/libopengdroid.so")
PY

echo "==> aligning"
"$ZIPALIGN" -p -f 4 build/app-unsigned.apk build/app-aligned.apk

echo "==> signing"
"$JAVA_HOME/bin/java" -jar "$(cygpath -w "$APKSIGNER_JAR" 2>/dev/null || echo "$APKSIGNER_JAR")" sign \
    --ks opengdroid.jks --ks-pass pass:opengdroid --key-pass pass:opengdroid \
    --ks-key-alias opengdroid --min-sdk-version "$MIN_SDK" \
    --out build/app-signed.apk build/app-aligned.apk

echo "==> done: $(pwd)/build/app-signed.apk"
