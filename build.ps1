# Build a release and the per-user installer (no admin, no dependencies).
# Usage: ./build.ps1 -Version 1.0.1 [-Publish]   (Publish uploads to GitHub Releases with your gh login)
param([Parameter(Mandatory)][string]$Version, [switch]$Publish)
# native tools write progress to stderr; check exit codes instead of treating stderr as failure
function Check { if ($LASTEXITCODE -ne 0) { throw "step failed ($LASTEXITCODE)" } }
if (-not (Get-Command vpk -ErrorAction SilentlyContinue)) { dotnet tool install -g vpk }
# bump only the package version (the first `version =` line), not dependency versions
$toml = Get-Content app/Cargo.toml -Raw
[IO.File]::WriteAllText("$PWD/app/Cargo.toml", ([regex]'(?m)^version = ".*"').Replace($toml, "version = `"$Version`"", 1))
cargo build --release -p floatlyrics 2>&1 | Out-Host; Check
Remove-Item dist -Recurse -ErrorAction SilentlyContinue; New-Item -ItemType Directory dist | Out-Null
Copy-Item target/release/FloatLyrics.exe dist/
vpk pack --packId FloatLyrics --packVersion $Version --packDir dist --mainExe FloatLyrics.exe --packTitle FloatLyrics --runtime win-x64 --icon app/ui/icon.ico --outputDir releases 2>&1 | Out-Host; Check
if ($Publish) {
  vpk upload github --repoUrl https://github.com/awan-gh/FloatLyrics --token (gh auth token) --publish --releaseName "FloatLyrics $Version" --tag "v$Version" --outputDir releases 2>&1 | Out-Host; Check
}
