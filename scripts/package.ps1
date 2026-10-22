# Build a release of NanaType or NanaType Editor and package it.
#   pwsh scripts/package.ps1 [-Product app|editor] [-Portable] [-SkipBuild] [-Profile dist|dist-small]
#   pwsh scripts/package.ps1 -Steam [-SkipBuild] [-Profile ...]   # the Steam depot (docs/steam.md)
# Each product builds only its own crate, so neither package contains the
# other's code, strings or assets.
# Needs: nana-packager on PATH (cargo install --path <NanaUI>/crates/nana-packager),
# Inno Setup 6 (iscc) for the installer. This script produces unsigned artifacts.
param(
    [ValidateSet("app", "editor")][string]$Product = "app",
    [switch]$Portable,
    [switch]$SkipBuild,
    [ValidateSet("dist", "dist-small")][string]$Profile = "dist",
    [switch]$Steam
)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
if ($Steam -and ($Portable -or $Product -eq "editor")) {
    throw "-Steam packages NanaType's Steam depot; it combines with neither -Portable nor -Product editor"
}
# The Steam edition updates through SteamPipe and leaves the unofficial chat
# sources out; it shares the GitHub release's settings folders.
$features = if ($Steam) { @("--no-default-features", "--features", "builtin-fonts,steam") } else { @() }

# A Steam id from the environment, else from the gitignored .cargo/config.toml [env].
function Steam-Id([string]$name) {
    $value = [Environment]::GetEnvironmentVariable($name)
    $local = Join-Path $root ".cargo\config.toml"
    if (-not $value -and (Test-Path $local)) {
        $line = Select-String -Path $local -Pattern "^\s*$name\s*=\s*`"?(\d+)`"?" | Select-Object -First 1
        if ($line) { $value = $line.Matches.Groups[1].Value }
    }
    if ($value -notmatch '^\d+$' -or [uint64]$value -eq 0) {
        throw "$name is not set: add it to .cargo/config.toml [env] (see .cargo/config.example.toml)"
    }
    return $value
}
if ($Steam) {
    # Fail before building: the release build itself refuses to guess the app.
    $null = Steam-Id "NANATYPE_STEAM_APP_ID"
    $null = Steam-Id "NANATYPE_STEAM_DEPOT_WINDOWS"
}
if ($Product -eq "editor") {
    $package = "nanatype-editor"; $binary = "nanatype-editor.exe"; $folder = "NanaTypeEditor"
    $display = "NanaType Editor"; $config = Join-Path $root "packaging\editor\nana-package.toml"
    $installer = Join-Path $root "packaging\nanatype-editor.iss"
} else {
    $package = "nanatype"; $binary = "nanatype.exe"; $folder = "NanaType"
    $display = "NanaType"; $config = Join-Path $root "packaging\nana-package.toml"
    $installer = Join-Path $root "packaging\nanatype.iss"
}

$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches.Groups[1].Value
if (-not $env:NANATYPE_BUILD_ID) { $env:NANATYPE_BUILD_ID = (git rev-parse --short HEAD) }
if (-not $SkipBuild) {
    $env:NANATYPE_REQUIRE_RESOURCES = "1"
    # Static C runtime: the package must not need the VC++ redistributable.
    $env:RUSTFLAGS = "-C target-feature=+crt-static"
    # A local build.incremental=true must not affect the release artifact.
    $previousIncremental = $env:CARGO_INCREMENTAL
    try {
        $env:CARGO_INCREMENTAL = "0"
        cargo build --profile $Profile -p $package --locked @features
        if ($LASTEXITCODE -ne 0) { throw "Release build failed ($LASTEXITCODE)" }
    } finally {
        $env:CARGO_INCREMENTAL = $previousIncremental
    }
}
$exe = Join-Path $root "target\$Profile\$binary"

# DX12 shader compiler shipped beside the exe (open-source DXC, see
# THIRD_PARTY_NOTICES.md). Set DXC_DLL to an official release's
# bin\x64\dxcompiler.dll; otherwise the Windows SDK copy is used.
$dxc = $env:DXC_DLL
if (-not $dxc) {
    $dxc = Get-ChildItem "C:\Program Files (x86)\Windows Kits\10\bin\*\x64\dxcompiler.dll" |
        Sort-Object FullName | Select-Object -Last 1 -ExpandProperty FullName
}
New-Item -ItemType Directory -Force (Join-Path $root "target\dxc") | Out-Null
Copy-Item $dxc (Join-Path $root "target\dxc\dxcompiler.dll") -Force

# Licences, staged in target\licenses for the installer and the zip: the
# notices (checked against Cargo.lock), each Rust crate's licence files, and
# a DXC release's own licence files (bin\x64\ -> two levels up).
$staged = Join-Path $root "target\licenses"
Remove-Item -Recurse -Force $staged -ErrorAction SilentlyContinue
if ($Steam) {
    pwsh (Join-Path $PSScriptRoot "third-party-notices.ps1") -Steam -Texts $staged
} else {
    pwsh (Join-Path $PSScriptRoot "third-party-notices.ps1") -Check -Texts $staged -Package $package
}
if ($LASTEXITCODE -ne 0) { throw "Third-party notices validation failed ($LASTEXITCODE)" }
if ($env:DXC_DLL) {
    $release = Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $env:DXC_DLL))
    $dxcLicenses = Get-ChildItem (Join-Path $release "LICENSE*") -File -ErrorAction SilentlyContinue
    if (-not $dxcLicenses) { throw "No LICENSE* files in $release for the DXC release named by DXC_DLL" }
    New-Item -ItemType Directory -Force (Join-Path $staged "dxc") | Out-Null
    Copy-Item $dxcLicenses (Join-Path $staged "dxc")
}
& (Join-Path $PSScriptRoot "package-licenses.ps1") -Staged $staged -Product $Product -Steam:$Steam
# Outside target/: NanaUI treats executables under a cargo target dir as dev builds.
$out = Join-Path $root "dist"
Remove-Item -Recurse -Force $out -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $out | Out-Null

if ($Steam) {
    # Steamworks loads steam_api64.dll from beside the executable: take the
    # copy of the steamworks-sys this build linked.
    $meta = cargo metadata --format-version 1 --offline --locked | ConvertFrom-Json -Depth 64
    $sys = $meta.packages | Where-Object { $_.name -eq "steamworks-sys" } | Select-Object -First 1
    if (-not $sys) { throw "steamworks-sys is not in Cargo.lock" }
    $sdk = if ($env:STEAM_SDK_LOCATION) { $env:STEAM_SDK_LOCATION } else { Join-Path (Split-Path -Parent $sys.manifest_path) "lib\steam" }
    $steamDir = Join-Path $root "target\steam"
    New-Item -ItemType Directory -Force $steamDir | Out-Null
    Copy-Item (Join-Path $sdk "redistributable_bin\win64\steam_api64.dll") (Join-Path $steamDir "steam_api64.dll") -Force
    $template = Get-Content -Raw (Join-Path $root "packaging\steam\nana-package.steam.toml")
    $filled = $template.Replace("@ROOT@", ($root -replace '\\', '/'))
    $filled = $filled.Replace("@VERSION@", $version)
    $filled = $filled.Replace("@APP_ID@", (Steam-Id "NANATYPE_STEAM_APP_ID"))
    $filled = $filled.Replace("@DEPOTS@", "windows = $(Steam-Id 'NANATYPE_STEAM_DEPOT_WINDOWS')")
    $config = Join-Path $steamDir "nana-package.toml"
    [IO.File]::WriteAllText($config, $filled, [Text.UTF8Encoding]::new($false))
}
if ($Portable) {
    # As deep below the repository as the original, so its relative sources resolve.
    $portableDir = if ($Product -eq "editor") { Join-Path $out "editor" } else { $out }
    New-Item -ItemType Directory -Force $portableDir | Out-Null
    $portable = Join-Path $portableDir "nana-package.toml"
    (Get-Content $config) -replace 'backend = "installer"', 'backend = "portable"' | Set-Content $portable
    $config = $portable
}
nana-packager package --config $config --exe $exe --out $out --profile $Profile --build-id $env:NANATYPE_BUILD_ID
if ($LASTEXITCODE -ne 0) { throw "Package creation failed ($LASTEXITCODE)" }
$app = Join-Path $out "app\$folder"
nana-packager validate $app --run
if ($LASTEXITCODE -ne 0) { throw "Package validation failed ($LASTEXITCODE)" }

if ($Steam) {
    # SteamPipe uploads the depot from dist\steam\depot_build_<depot>.vdf.
    Write-Host "Packaged the Steam depot of $display $version into $out (see $(Join-Path $out 'steam'))"
    exit 0
}
if ($Portable) {
    # The startup self-check creates portable user data, including diagnostics.
    # A fresh release carries the application payload and creates its own data.
    $payload = Get-ChildItem -LiteralPath $app -Force |
        Where-Object { $_.Name -ne "data" } |
        Select-Object -ExpandProperty FullName
    Compress-Archive -LiteralPath $payload -DestinationPath (Join-Path $out "$folder-$version-portable.zip")
} else {
    iscc "/DAppVersion=$version" "/DSourceDir=$app" "/DOutputDir=$out" $installer
    if ($LASTEXITCODE -ne 0) { throw "Installer compilation failed ($LASTEXITCODE)" }
}
Write-Host "Packaged $display $version into $out"
