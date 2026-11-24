# Builds SwTsM.exe and the SwTsM-Setup.exe installer into .\dist
Set-Location $PSScriptRoot

cargo build --release -p swtsm
if ($LASTEXITCODE -ne 0) { throw "app build failed" }

cargo build --release -p swtsm-setup
if ($LASTEXITCODE -ne 0) { throw "installer build failed" }

New-Item -ItemType Directory -Force dist | Out-Null
Copy-Item target\release\SwTsM.exe dist\SwTsM.exe -Force
Copy-Item target\release\SwTsM-Setup.exe dist\SwTsM-Setup.exe -Force
Write-Host "Done: dist\SwTsM-Setup.exe"
