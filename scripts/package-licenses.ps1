# Collect the complete, readable redistribution notices before nana-packager
# inventories and signs the application. No files are appended after sealing.
param(
    [string]$Staged = (Join-Path (Split-Path -Parent $PSScriptRoot) "target\licenses"),
    [string]$Destination
)
$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent $PSScriptRoot
if (-not $Destination) { $Destination = Join-Path $Staged "THIRD_PARTY_LICENSES.txt" }
$destinationPath = [IO.Path]::GetFullPath($Destination)
$licenseSources = [ordered]@{}

function Add-LicenseSource([string]$Name, [string]$Path, [string]$Category) {
    if ([IO.Path]::GetFullPath($Path) -eq $destinationPath) { return }
    if ($licenseSources.Contains($Name)) {
        $old = [IO.File]::ReadAllText($licenseSources[$Name]).Replace("`r`n", "`n")
        $new = [IO.File]::ReadAllText($Path).Replace("`r`n", "`n")
        if ($old -eq $new) { return }
        $Name = "$Category/$Name"
    }
    if ($licenseSources.Contains($Name)) { throw "Duplicate licence section: $Name" }
    $licenseSources[$Name] = $Path
}

foreach ($name in @("LICENSE", "LICENSE-MIT", "THIRD_PARTY_NOTICES.md")) {
    Add-LicenseSource $name (Join-Path $repoRoot $name) "project"
}
foreach ($name in @("WASMOON-LICENSE", "LUA-LICENSE")) {
    Add-LicenseSource $name (Join-Path $repoRoot "crates\plugins\runtime\$name") "plugins"
}
foreach ($file in (Get-ChildItem (Join-Path $repoRoot "crates\theme\fonts\*.LICENSE.txt") -File | Sort-Object Name)) {
    Add-LicenseSource $file.Name $file.FullName "fonts"
}
foreach ($folder in @((Join-Path $repoRoot "packaging\licenses"), $Staged)) {
    foreach ($file in (Get-ChildItem -LiteralPath $folder -File -Recurse | Sort-Object FullName)) {
        $name = [IO.Path]::GetRelativePath($folder, $file.FullName).Replace('\', '/')
        Add-LicenseSource $name $file.FullName "packaging"
    }
}

$text = [Text.StringBuilder]::new()
[void]$text.AppendLine("NanaType redistribution licences and notices")
[void]$text.AppendLine("Every named section below retains the complete source text.")
[void]$text.AppendLine()
foreach ($entry in $licenseSources.GetEnumerator()) {
    [void]$text.AppendLine("===== BEGIN $($entry.Key) =====")
    [void]$text.Append([IO.File]::ReadAllText($entry.Value))
    [void]$text.AppendLine()
    [void]$text.AppendLine("===== END $($entry.Key) =====")
    [void]$text.AppendLine()
}
New-Item -ItemType Directory -Force (Split-Path -Parent $destinationPath) | Out-Null
[IO.File]::WriteAllText($destinationPath, $text.ToString(), [Text.UTF8Encoding]::new($false))
Write-Output "Collected $($licenseSources.Count) complete licence sections into $destinationPath"
