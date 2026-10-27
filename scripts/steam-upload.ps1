# Upload the Steam depots packaged by scripts/package.ps1 -Steam or
# scripts/package-macos.sh --steam with SteamCMD (docs/steam.md).
#   pwsh scripts/steam-upload.ps1 -Account <build account> [-Platform windows|macos|all] [-Preview]
#       [-WindowsContent <dir>] [-MacosContent <dir>]
# The account can also come from STEAM_BUILD_ACCOUNT; steamcmd comes from
# STEAMCMD or PATH. steamcmd asks for the password and Steam Guard code itself
# and caches the login; this script never reads, takes or stores a password.
# The build is not set live: do that on the Steamworks builds page.
param(
    [string]$Account = $env:STEAM_BUILD_ACCOUNT,
    [ValidateSet("windows", "macos", "all")][string]$Platform = $(if ($IsMacOS) { "macos" } else { "windows" }),
    [string]$WindowsContent,
    [string]$MacosContent,
    [switch]$Preview
)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
if (-not $Account) { throw "No build account: pass -Account <name> or set STEAM_BUILD_ACCOUNT" }

# A Steam id from the environment, else from the gitignored .cargo/config.toml [env].
function Steam-Id([string]$name) {
    $value = [Environment]::GetEnvironmentVariable($name)
    $local = Join-Path $root ".cargo/config.toml"
    if (-not $value -and (Test-Path $local)) {
        $line = Select-String -Path $local -Pattern "^\s*$name\s*=\s*`"?(\d+)`"?" | Select-Object -First 1
        if ($line) { $value = $line.Matches.Groups[1].Value }
    }
    if ($value -notmatch '^\d+$' -or [uint64]$value -eq 0) {
        throw "$name is not set: add it to .cargo/config.toml [env] (see .cargo/config.example.toml)"
    }
    return $value
}
$appId = Steam-Id "NANATYPE_STEAM_APP_ID"
$app = Join-Path $root "dist/app"
if (-not $WindowsContent) { $WindowsContent = Join-Path $app "NanaType" }
if (-not $MacosContent) { $MacosContent = $app }

# Each platform: its depot and the folder that becomes the depot root.
$depots = @()
if ($Platform -in "windows", "all") {
    if (-not (Test-Path -PathType Leaf (Join-Path $WindowsContent "NanaType.exe"))) {
        throw "No NanaType.exe in ${WindowsContent}: run pwsh scripts/package.ps1 -Steam first"
    }
    $depots += [pscustomobject]@{ Name = "windows"; Id = (Steam-Id "NANATYPE_STEAM_DEPOT_WINDOWS"); Content = (Resolve-Path $WindowsContent).Path }
}
if ($Platform -in "macos", "all") {
    if (-not (Test-Path -PathType Container (Join-Path $MacosContent "NanaType.app"))) {
        throw "No NanaType.app in ${MacosContent}: run scripts/package-macos.sh --steam first"
    }
    $depots += [pscustomobject]@{ Name = "macos"; Id = (Steam-Id "NANATYPE_STEAM_DEPOT_MACOS"); Content = (Resolve-Path $MacosContent).Path }
}

# Generated scripts and SteamPipe output stay under the gitignored target/.
$dir = Join-Path $root "target/steampipe"
$output = Join-Path $dir "output"
New-Item -ItemType Directory -Force $output | Out-Null
Remove-Item (Join-Path $dir "*.vdf") -ErrorAction SilentlyContinue
$utf8 = [Text.UTF8Encoding]::new($false)
$depotTemplate = Get-Content -Raw (Join-Path $root "packaging/steam/depot_build.vdf.in")
$lines = @()
foreach ($depot in $depots) {
    $filled = $depotTemplate.Replace("@DEPOT_ID@", $depot.Id).Replace("@CONTENT_ROOT@", $depot.Content)
    [IO.File]::WriteAllText((Join-Path $dir "depot_build_$($depot.Id).vdf"), $filled, $utf8)
    $lines += "`t`t`"$($depot.Id)`" `"depot_build_$($depot.Id).vdf`""
}
$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches.Groups[1].Value
$commit = git rev-parse --short HEAD
$desc = "NanaType $version ($commit) $(($depots | ForEach-Object Name) -join '+')"
$filled = (Get-Content -Raw (Join-Path $root "packaging/steam/app_build.vdf.in")).
    Replace("@APP_ID@", $appId).Replace("@DESC@", $desc).Replace("@CONTENT_ROOT@", $app).
    Replace("@BUILD_OUTPUT@", $output).Replace("@PREVIEW@", $(if ($Preview) { "1" } else { "0" })).
    Replace("@DEPOTS@", ($lines -join "`n"))
$vdf = Join-Path $dir "app_build_$appId.vdf"
[IO.File]::WriteAllText($vdf, $filled, $utf8)

$steamcmd = if ($env:STEAMCMD) { $env:STEAMCMD } else { "steamcmd" }
Write-Host "Uploading $desc with $vdf"
& $steamcmd +login $Account +run_app_build $vdf +quit
if ($LASTEXITCODE -ne 0) { throw "steamcmd failed ($LASTEXITCODE); logs in $output" }
if ($Preview) {
    Write-Host "Preview only: nothing was uploaded; the file lists are in $output"
} else {
    Write-Host "Uploaded. Set the build live at https://partner.steamgames.com/apps/builds/$appId"
}
