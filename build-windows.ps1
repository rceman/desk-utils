# Rebuild and redeploy diskhound.exe + refresh desktop shortcuts (run from Windows PowerShell)
$env:CARGO_TARGET_DIR = 'C:\Tools\diskhound-build'
Set-Location '\\wsl.localhost\Ubuntu\home\therceman\git\diskhound'
cargo build --release
$exe = 'C:\Tools\diskhound\diskhound.exe'
Copy-Item 'C:\Tools\diskhound-build\release\diskhound.exe' $exe -Force

# manifest for themed common controls: embed via mt.exe (SDK) or ship loose file
$manifest = 'C:\Tools\diskhound\diskhound.exe.manifest'
Copy-Item '\\wsl.localhost\Ubuntu\home\therceman\git\diskhound\diskhound.exe.manifest' $manifest -Force
$mt = Get-ChildItem 'C:\Program Files (x86)\Windows Kits\10\bin' -Recurse -Filter mt.exe -ErrorAction SilentlyContinue |
      Where-Object { $_.FullName -like '*x64*' } | Select-Object -First 1
if ($mt) {
    & $mt.FullName -nologo -manifest $manifest "-outputresource:$exe;#1"
    Write-Output 'manifest embedded'
} else {
    Write-Output 'manifest shipped loose'
}

$desktop = [Environment]::GetFolderPath('Desktop')
$ws = New-Object -ComObject WScript.Shell

$scan = $ws.CreateShortcut("$desktop\diskhound.lnk")
$scan.TargetPath = $exe
$scan.Arguments = ''
$scan.WorkingDirectory = 'C:\Tools\diskhound'
$scan.IconLocation = "$exe,0"
$scan.Description = 'diskhound - find where disk space went'
$scan.Save()

Write-Output "installed: $exe"
Write-Output "shortcut:  $desktop\diskhound.lnk"
