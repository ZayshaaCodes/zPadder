# Builds the portable ZPadder release zip in dist\, with SHA-256 checksums.
#
#   powershell -ExecutionPolicy Bypass -File scripts\package.ps1 [-AllowDirty] [-SkipTests]
#
# Needs cargo-about for the third-party licence notice:
#   cargo install cargo-about --locked --features cli
#
# The build uses its own target folder (target\package), so a running zpadder.exe from
# target\release doesn't block it. Code signing happens in the release workflow
# ("Releasing" in .github/CONTRIBUTING.md); a zip built here is unsigned.

param(
    # Package even when the working tree has uncommitted changes.
    [switch]$AllowDirty,
    # Skip `cargo test` before building.
    [switch]$SkipTests
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

function Invoke-Checked([string]$what, [scriptblock]$command) {
    & $command
    if ($LASTEXITCODE -ne 0) { throw "$what failed" }
}

# A release is built from committed sources only.
$hasCommits = (& git rev-parse --verify --quiet HEAD 2>$null) -and $LASTEXITCODE -eq 0
if ($hasCommits) {
    $dirty = & git status --porcelain
    if ($dirty -and -not $AllowDirty) {
        throw "The working tree has uncommitted changes. Commit them, or pass -AllowDirty for a test build."
    }
} elseif (-not $AllowDirty) {
    throw "The repository has no commits yet. Pass -AllowDirty for a test build."
}

if (-not (Get-Command cargo-about -ErrorAction SilentlyContinue)) {
    throw "cargo-about is missing: cargo install cargo-about --locked --features cli"
}

if (-not $SkipTests) {
    Invoke-Checked 'cargo test' { cargo test --workspace --locked }
}
$env:CARGO_TARGET_DIR = Join-Path $root 'target\package'
try {
    Invoke-Checked 'cargo build' { cargo build --release --locked -p zpadder }
} finally {
    Remove-Item Env:\CARGO_TARGET_DIR
}
$bin = Join-Path $root 'target\package\release'

$metadata = cargo metadata --format-version 1 --no-deps --locked | ConvertFrom-Json
$version = ($metadata.packages | Where-Object { $_.name -eq 'zpadder' }).version
$name = "zpadder-$version-windows-x64"
$dist = Join-Path $root 'dist'
$stage = Join-Path $dist $name
foreach ($old in @($stage, "$stage.zip", "$dist\$name-symbols.zip", "$dist\SHA256SUMS.txt")) {
    if (Test-Path $old) { Remove-Item -Recurse -Force $old }
}
New-Item -ItemType Directory -Force "$stage\profiles", "$stage\docs", "$stage\licenses" | Out-Null

# Text files are written as UTF-8 without a byte-order mark, with Windows line endings.
$utf8 = New-Object System.Text.UTF8Encoding($false)
function Write-Text($path, $lines) { [System.IO.File]::WriteAllLines($path, [string[]]$lines, $utf8) }

# In the zip, example profiles sit in profiles\ and the schema at the top.
function Copy-Doc($source, $target) {
    $lines = (Get-Content $source -Encoding UTF8) -replace '\.\./profiles/examples/', '../profiles/'
    Write-Text $target $lines
}

Copy-Item "$bin\zpadder.exe", "$bin\zpadder-cli.exe" $stage
Copy-Item LICENSE-MIT, LICENSE-APACHE $stage
Write-Text "$stage\README.txt" ((Get-Content docs\release-readme.txt -Encoding UTF8) -replace '\{version\}', $version)
Copy-Item CHANGELOG.md $stage
Copy-Doc docs\profiles.md "$stage\docs\profiles.md"
Copy-Doc docs\xpadder-import.md "$stage\docs\xpadder-import.md"
Copy-Doc docs\anti-cheat.md "$stage\docs\anti-cheat.md"
Copy-Item schema\profile.schema.json $stage
# The credits without the note about the maintainer's unprocessed original, which isn't shipped.
Write-Text "$stage\docs\controller-pictures-credits.md" (
    (Get-Content assets\controllers\CREDITS.md -Encoding UTF8) | Where-Object { $_ -notmatch 'originals' }
)
foreach ($profile in Get-ChildItem profiles\examples\*.toml) {
    $lines = (Get-Content $profile.FullName -Encoding UTF8) -replace '^#:schema .*', '#:schema ../profile.schema.json'
    Write-Text "$stage\profiles\$($profile.Name)" $lines
}

# Third-party licences: Rust crates from cargo-about, plus the C code compiled into SDL.
Invoke-Checked 'cargo about' {
    cargo about generate --locked --fail -c scripts\about.toml scripts\about.hbs -o "$stage\THIRD_PARTY_LICENSES.html"
}
$sdlSource = ($(cargo metadata --format-version 1 --locked | ConvertFrom-Json).packages |
    Where-Object { $_.name -eq 'sdl3-src' } | Select-Object -First 1).manifest_path | Split-Path
if (-not $sdlSource) { throw 'cannot find the sdl3-src sources for their licence files' }
Copy-Item "$sdlSource\SDL\LICENSE.txt" "$stage\licenses\SDL3.txt"
# hidapi offers GPL-3, BSD or its original licence; ZPadder uses it under the BSD licence.
Copy-Item "$sdlSource\SDL\src\hidapi\LICENSE-bsd.txt" "$stage\licenses\hidapi-BSD.txt"

$zip = "$stage.zip"
Compress-Archive -Path $stage -DestinationPath $zip -Force
$zips = @($zip)

# Debug symbols, for reading crash reports, as a separate download.
$pdbs = Get-ChildItem "$bin\zpadder.pdb", "$bin\zpadder_cli.pdb" -ErrorAction SilentlyContinue
if ($pdbs) {
    $symbols = "$dist\$name-symbols.zip"
    Compress-Archive -Path $pdbs.FullName -DestinationPath $symbols -Force
    $zips += $symbols
}

$sums = foreach ($file in $zips) {
    $hash = (Get-FileHash -Algorithm SHA256 $file).Hash.ToLowerInvariant()
    "$hash  $(Split-Path -Leaf $file)"
}
Write-Text "$dist\SHA256SUMS.txt" $sums
Write-Host "Built:"
$zips + "$dist\SHA256SUMS.txt" | ForEach-Object { Write-Host "  $_" }
