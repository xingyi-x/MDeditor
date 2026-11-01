@echo off
rem Build MDeditor release exe. Requires: Rust toolchain already installed by setup.
setlocal
set ROOT=%~dp0
set "PATH=%ROOT%_toolchain\cargo\bin;%PATH%"
set "CARGO_HOME=%ROOT%_toolchain\cargo"
set "RUSTUP_HOME=%ROOT%_toolchain\rustup"
cd /d "%ROOT%src-tauri"
cargo build --release
if errorlevel 1 (
  echo.
  echo Build FAILED. See messages above.
  pause
  exit /b 1
)
echo.
echo Build OK. Output exe:
dir /b "target\release\mdeditor.exe"
echo Full path: %ROOT%src-tauri\target\release\mdeditor.exe
pause
