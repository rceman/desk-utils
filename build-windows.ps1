# Rebuild and redeploy diskhound.exe + refresh desktop shortcuts (run from Windows PowerShell)
$env:CARGO_TARGET_DIR = 'C:\Tools\diskhound-build'
Set-Location '\\wsl.localhost\Ubuntu\home\therceman\git\diskhound'
cargo build --release
Copy-Item 'C:\Tools\diskhound-build\release\diskhound.exe' 'C:\Tools\diskhound\diskhound.exe' -Force
Write-Output 'installed: C:\Tools\diskhound\diskhound.exe'

$desktop = [Environment]::GetFolderPath('Desktop')
$ws = New-Object -ComObject WScript.Shell

$scan = $ws.CreateShortcut("$desktop\diskhound.lnk")
$scan.TargetPath = 'C:\Tools\diskhound\diskhound.exe'
$scan.Arguments = 'C:\'
$scan.WorkingDirectory = 'C:\Tools\diskhound'
$scan.IconLocation = 'C:\Tools\diskhound\diskhound.exe,0'
$scan.Description = 'diskhound - find where disk space went'
$scan.Save()

$compact = $ws.CreateShortcut("$desktop\diskhound compact.lnk")
$compact.TargetPath = 'C:\Tools\diskhound\diskhound.exe'
$compact.Arguments = 'compact'
$compact.WorkingDirectory = 'C:\Tools\diskhound'
$compact.IconLocation = 'C:\Tools\diskhound\diskhound.exe,0'
$compact.Description = 'diskhound - shut down WSL and shrink its virtual disks'
$compact.Save()

Write-Output "shortcuts: $desktop\diskhound.lnk, $desktop\diskhound compact.lnk"
