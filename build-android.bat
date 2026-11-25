@echo off
REM Build OpenGDroid for Android. Run from the repo root.
REM MSYS path translation is bypassed because this is a native cmd script.
setlocal

set "PATH=C:\Users\AT\.cargo\bin;%PATH%"
set "CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="
set "CC_aarch64_linux_android="

cd /d "%~dp0"

echo === aarch64-linux-android ===
cargo build --release --target aarch64-linux-android
if errorlevel 1 goto :fail

echo.
echo === artifacts ===
dir /b target\aarch64-linux-android\release\*.so
goto :eof

:fail
echo BUILD FAILED
exit /b 1
