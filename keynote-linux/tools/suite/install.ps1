# Install (or update) the whole Bramble Office suite - Keydeck, Letters and Values - on Windows,
# from the latest GitHub release, with Start menu and desktop shortcuts. In PowerShell:
#   irm https://raw.githubusercontent.com/bramble-apps/bramble-office/main/install.ps1 | iex
# Remove it again:
#   & ([scriptblock]::Create((irm https://raw.githubusercontent.com/bramble-apps/bramble-office/main/install.ps1))) -Uninstall
# Nothing needs administrator rights; run it again to update.
param([switch]$Uninstall)
$ErrorActionPreference = 'Stop'

$repo = 'bramble-apps/bramble-office'
$dest = Join-Path $env:LOCALAPPDATA 'Programs\Bramble Office'
$menu = Join-Path ([Environment]::GetFolderPath('Programs')) 'Bramble Office'
$desk = [Environment]::GetFolderPath('Desktop')
$apps = @(@{ Name = 'Keydeck'; Exe = 'keydeck.exe' }, @{ Name = 'Letters'; Exe = 'letters.exe' }, @{ Name = 'Values'; Exe = 'values.exe' })

if ($Uninstall) {
    foreach ($a in $apps) { Remove-Item -Force -ErrorAction SilentlyContinue (Join-Path $desk "$($a.Name).lnk") }
    Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $menu, $dest
    Write-Host 'Bramble Office removed (your documents are untouched).'
    return
}

$tmp = Join-Path ([IO.Path]::GetTempPath()) ("bramble-" + [Guid]::NewGuid())
New-Item -ItemType Directory -Force $tmp | Out-Null
try {
    Write-Host 'Downloading Bramble Office for Windows...'
    $zip = Join-Path $tmp 'suite.zip'
    $ProgressPreference = 'SilentlyContinue'   # (the progress bar makes downloads very slow)
    Invoke-WebRequest "https://github.com/$repo/releases/latest/download/Bramble-Office-windows-x86_64.zip" -OutFile $zip -UseBasicParsing
    Expand-Archive $zip -DestinationPath $tmp -Force
    if (Test-Path $dest) { Remove-Item -Recurse -Force $dest }
    New-Item -ItemType Directory -Force (Split-Path $dest) | Out-Null
    Move-Item (Join-Path $tmp 'bramble-office') $dest
    Get-ChildItem $dest -Recurse | Unblock-File

    New-Item -ItemType Directory -Force $menu | Out-Null
    $shell = New-Object -ComObject WScript.Shell
    foreach ($a in $apps) {
        $exe = Join-Path $dest $a.Exe
        foreach ($dir in @($menu, $desk)) {
            $lnk = $shell.CreateShortcut((Join-Path $dir "$($a.Name).lnk"))
            $lnk.TargetPath = $exe
            $lnk.WorkingDirectory = $dest
            $lnk.IconLocation = "$exe,0"
            $lnk.Save()
        }
        Write-Host "  installed $($a.Name)"
    }
    Write-Host ''
    Write-Host 'Bramble Office is installed: Keydeck, Letters and Values are in the Start menu and on the desktop.'
} finally {
    Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $tmp
}
