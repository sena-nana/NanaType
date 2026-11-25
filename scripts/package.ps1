# Build a release of NanaType or NanaType Editor and package it.
#   pwsh scripts/package.ps1 [-Product app|editor] [-Portable] [-SkipBuild] [-Profile dist|dist-small]
# Each product builds only its own crate, so neither package contains the
# other's code, strings or assets.
# Needs: nana-packager on PATH (cargo install --path <NanaUI>/crates/nana-packager),
# Inno Setup 6 (iscc) for the installer. This script produces unsigned artifacts.
param(
    [ValidateSet("app", "editor")][string]$Product = "app",
    [switch]$Portable,
    [switch]$SkipBuild,
    [ValidateSet("dist", "dist-small")][string]$Profile = "dist"
)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
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
    # Keep the embedded recognizer portable across supported x64 PCs. The
    # upstream default specializes to the build machine's CPU.
    foreach ($flag in @("GGML_NATIVE", "GGML_SSE42", "GGML_AVX", "GGML_AVX2", "GGML_BMI2", "GGML_FMA", "GGML_F16C")) {
        [Environment]::SetEnvironmentVariable($flag, "OFF", "Process")
    }
    # Static C runtime: the package must not need the VC++ redistributable.
    $env:RUSTFLAGS = "-C target-feature=+crt-static"
    # A local build.incremental=true must not affect the release artifact.
    $previousIncremental = $env:CARGO_INCREMENTAL
    try {
        $env:CARGO_INCREMENTAL = "0"
        cargo build --profile $Profile -p $package --locked
        if ($LASTEXITCODE -ne 0) { throw "Release build failed ($LASTEXITCODE)" }
    } finally {
        $env:CARGO_INCREMENTAL = $previousIncremental
    }
}
$exe = Join-Path $root "target\$Profile\$binary"
# The `cargo metadata --offline` calls below read the whole locked graph,
# including crates this build never compiled (other platforms, dev-only):
# download them all first, or a clean machine such as CI cannot list them.
cargo fetch --locked
if ($LASTEXITCODE -ne 0) { throw "cargo fetch failed ($LASTEXITCODE)" }

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
pwsh (Join-Path $PSScriptRoot "third-party-notices.ps1") -Check -Texts $staged -Package $package
if ($LASTEXITCODE -ne 0) { throw "Third-party notices validation failed ($LASTEXITCODE)" }
if ($env:DXC_DLL) {
    $release = Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $env:DXC_DLL))
    $dxcLicenses = Get-ChildItem (Join-Path $release "LICENSE*") -File -ErrorAction SilentlyContinue
    if (-not $dxcLicenses) { throw "No LICENSE* files in $release for the DXC release named by DXC_DLL" }
    New-Item -ItemType Directory -Force (Join-Path $staged "dxc") | Out-Null
    Copy-Item $dxcLicenses (Join-Path $staged "dxc")
}
& (Join-Path $PSScriptRoot "package-licenses.ps1") -Staged $staged -Product $Product
# The built-in skins ship as a folder in the runtime resources, not in the
# executable (which keeps only the default skin, as a fallback). The SVG design
# masters and dotfiles stay in the repository: skins load only rendered PNGs.
$skins = Join-Path $root "target\skins"
Remove-Item -Recurse -Force $skins -ErrorAction SilentlyContinue
Copy-Item (Join-Path $root "crates\theme\builtin") $skins -Recurse
Get-ChildItem $skins -Recurse -Force -File |
    Where-Object { $_.Name.StartsWith(".") -or $_.Extension -eq ".svg" } |
    Remove-Item -Force
# Their fonts, each with its licence (docs/fonts.md).
$fonts = Join-Path $root "target\fonts"
Remove-Item -Recurse -Force $fonts -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $fonts | Out-Null
Copy-Item (Join-Path $root "crates\theme\fonts\*") $fonts -Include *.ttf, *.otf, *.LICENSE.txt
# Outside target/: NanaUI treats executables under a cargo target dir as dev builds.
$out = Join-Path $root "dist"
Remove-Item -Recurse -Force $out -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $out | Out-Null

if ($Portable) {
    # As deep below the repository as the original, so its relative sources resolve,
    # and outside dist/: nana-packager only writes into an empty directory.
    $portableDir = Join-Path $root $(if ($Product -eq "editor") { "target\editor" } else { "target" })
    New-Item -ItemType Directory -Force $portableDir | Out-Null
    # Not $portable: PowerShell names are case-insensitive, so that is the -Portable switch.
    $portableConfig = Join-Path $portableDir "nana-package.toml"
    (Get-Content $config) -replace 'backend = "installer"', 'backend = "portable"' | Set-Content $portableConfig
    $config = $portableConfig
}
nana-packager package --config $config --exe $exe --out $out --profile $Profile --build-id $env:NANATYPE_BUILD_ID
if ($LASTEXITCODE -ne 0) { throw "Package creation failed ($LASTEXITCODE)" }
$app = Join-Path $out "app\$folder"
nana-packager validate $app --run
if ($LASTEXITCODE -ne 0) { throw "Package validation failed ($LASTEXITCODE)" }

if ($Steam) {
    # scripts/steam-upload.ps1 uploads the depot with SteamPipe.
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
