# setup-env-permanent.ps1 — Run ONCE as regular user (no admin needed).
#
# Sets LIB, INCLUDE, CC, AR and adds LLVM + MSVC ARM64 to your user PATH
# permanently, so that "cargo build" / "cargo run" always work in any terminal
# or VS Code without dot-sourcing dev.ps1 first.
#
# Usage (PowerShell, from the project root — no elevation required):
#   .\setup-env-permanent.ps1
#
# After running, open a NEW terminal (or restart VS Code) for the changes
# to take effect.
# ─────────────────────────────────────────────────────────────────────────────

$msvc    = "C:\Program Files\Microsoft Visual Studio\18\Insiders\VC\Tools\MSVC\14.50.35717"
$sdk_lib = "C:\Program Files (x86)\Windows Kits\10\Lib\10.0.26100.0"
$sdk_inc = "C:\Program Files (x86)\Windows Kits\10\Include\10.0.26100.0"

$lib     = "$msvc\lib\arm64;$sdk_lib\um\arm64;$sdk_lib\ucrt\arm64"
$include = "$msvc\include;$sdk_inc\ucrt;$sdk_inc\um;$sdk_inc\shared"

[System.Environment]::SetEnvironmentVariable("LIB",     $lib,     "User")
[System.Environment]::SetEnvironmentVariable("INCLUDE", $include, "User")
[System.Environment]::SetEnvironmentVariable("CC",      "clang",  "User")
[System.Environment]::SetEnvironmentVariable("AR",      "llvm-ar","User")

# Add LLVM and MSVC ARM64 compiler to the user PATH (only if not already there)
$currentPath = [System.Environment]::GetEnvironmentVariable("PATH", "User")
$toAdd = @(
    "C:\Program Files\LLVM\bin",
    "$msvc\bin\Hostarm64\arm64"
)
foreach ($entry in $toAdd) {
    if ($currentPath -notlike "*$entry*") {
        $currentPath = "$entry;$currentPath"
    }
}
[System.Environment]::SetEnvironmentVariable("PATH", $currentPath, "User")

Write-Host ""
Write-Host "Done. Environment variables set permanently for your user account." -ForegroundColor Green
Write-Host ""
Write-Host "  LIB     = $lib"     -ForegroundColor DarkGray
Write-Host "  INCLUDE = (set)"    -ForegroundColor DarkGray
Write-Host "  CC      = clang"    -ForegroundColor DarkGray
Write-Host "  AR      = llvm-ar"  -ForegroundColor DarkGray
Write-Host ""
Write-Host "Next steps:" -ForegroundColor Cyan
Write-Host "  1. Close this terminal (and VS Code if open)"
Write-Host "  2. Reopen VS Code / a new terminal"
Write-Host "  3. cargo run -p dvd2chd-gui"
Write-Host ""
Write-Host "You never need to run dev.ps1 or this script again." -ForegroundColor Yellow
