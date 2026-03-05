# dev.ps1 — ARM64 Windows build environment
#
# Dot-source this script to set the required environment variables in your
# current PowerShell session, then use cargo normally:
#
#   . .\dev.ps1
#   cargo run -p dvd2chd-gui
#   cargo build --release
#   cargo clippy --workspace
#
# You only need to run this once per terminal session.
# ─────────────────────────────────────────────────────────────────────────────

# ── Adjust these two paths to match your installation ────────────────────────
$msvc = "C:\Program Files\Microsoft Visual Studio\18\Insiders\VC\Tools\MSVC\14.50.35717"
$sdk  = "C:\Program Files (x86)\Windows Kits\10\Lib\10.0.26100.0"
# ─────────────────────────────────────────────────────────────────────────────

$sdk_inc = "C:\Program Files (x86)\Windows Kits\10\Include\10.0.26100.0"

$env:LIB     = "$msvc\lib\arm64;$sdk\um\arm64;$sdk\ucrt\arm64"
$env:INCLUDE = "$msvc\include;$sdk_inc\ucrt;$sdk_inc\um;$sdk_inc\shared"
$env:PATH    = "$env:USERPROFILE\.cargo\bin;C:\Program Files\LLVM\bin;$msvc\bin\Hostarm64\arm64;$env:PATH"
$env:CC      = "clang"
$env:AR      = "llvm-ar"

Write-Host "ARM64 build environment set." -ForegroundColor Green
Write-Host "  MSVC : $msvc" -ForegroundColor DarkGray
Write-Host "  SDK  : $sdk" -ForegroundColor DarkGray
Write-Host ""
Write-Host "Run the app:  cargo run -p dvd2chd-gui" -ForegroundColor Cyan
