# Rebuild and redeploy diskhound.exe (run from Windows PowerShell)
$env:CARGO_TARGET_DIR = 'C:\Tools\diskhound-build'
Set-Location '\\wsl.localhost\Ubuntu\home\therceman\git\diskhound'
cargo build --release
Copy-Item 'C:\Tools\diskhound-build\release\diskhound.exe' 'C:\Tools\diskhound\diskhound.exe' -Force
Write-Output 'installed: C:\Tools\diskhound\diskhound.exe'
