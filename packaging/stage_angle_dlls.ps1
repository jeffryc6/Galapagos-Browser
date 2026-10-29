# packaging/stage_angle_dlls.ps1
# DELTA_05 A-I6: copia libEGL.dll y libGLESv2.dll (ANGLE, salida de `mozangle` con la
# característica `no-wgl` de Servo) junto a galapagos-content.exe. Sin ellas el worker termina
# con el código 7 y un WORKER_FAULT legible. d3dcompiler_47.dll se toma de System32.
#
# Uso (tras compilar el worker):
#   .\packaging\stage_angle_dlls.ps1 -Profile debug
#   .\packaging\stage_angle_dlls.ps1 -Profile release

[CmdletBinding()]
param (
    [ValidateSet("debug", "release")]
    [string]$Profile = "debug"
)

$ErrorActionPreference = "Stop"

$projectRoot = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$targetDir = Join-Path $projectRoot "crates\galapagos-content\target\$Profile"
$buildDir = Join-Path $targetDir "build"

if (-not (Test-Path $buildDir)) {
    Write-Error "No existe $buildDir. Compila primero el worker: cargo build --manifest-path crates/galapagos-content/Cargo.toml$(if ($Profile -eq 'release') { ' --release' })"
}

# Salida de mozangle más reciente que contenga ambas DLL
$outDirs = Get-ChildItem -Path $buildDir -Directory -Filter "mozangle-*" |
    ForEach-Object { Join-Path $_.FullName "out" } |
    Where-Object { (Test-Path (Join-Path $_ "libEGL.dll")) -and (Test-Path (Join-Path $_ "libGLESv2.dll")) } |
    Sort-Object { (Get-Item (Join-Path $_ "libEGL.dll")).LastWriteTime } -Descending

if (-not $outDirs) {
    Write-Error "No se encontraron libEGL.dll y libGLESv2.dll en $buildDir\mozangle-*\out. ¿Se compiló el worker con la característica 'no-wgl'?"
}

$source = @($outDirs)[0]
foreach ($dll in @("libEGL.dll", "libGLESv2.dll")) {
    Copy-Item (Join-Path $source $dll) -Destination $targetDir -Force
}

Write-Host "ANGLE copiado a $targetDir desde $source" -ForegroundColor Green
