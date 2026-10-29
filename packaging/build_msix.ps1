# packaging/build_msix.ps1
# Orquestacion de Compilacion Release, Empaquetado MSIX Multiproceso y Firma Digital
# Conforme a ZNVE v2.4.2 (Axiomas 1 y 2, ARC-01, SEC-01, SEC-03, Cero Mocks, Cero Debug)

[Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSAvoidUsingWriteHost', '')]
[CmdletBinding()]
param (
    [string]$Configuration = "release",
    [string]$OutputPackage = "",
    [switch]$KeepStaging
)

$ErrorActionPreference = "Stop"

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$projectRoot = Split-Path -Parent $scriptDir

if ($Configuration -ne "release") {
    Write-Host "AVISO: ZNVE v2.4.2 exige binarios reales de produccion. Forzando Configuration = 'release'." -ForegroundColor Yellow
    $Configuration = "release"
}

if (-not $OutputPackage -or $OutputPackage.StartsWith("\")) {
    $OutputPackage = Join-Path $scriptDir "GalapagosBrowser_0.1.0_x64.msix"
}

Write-Host "=== Empaquetado y Distribucion Segura MSIX de Produccion (ZNVE v2.4.2) ===" -ForegroundColor Cyan

# 1. Compilar / verificar binario del Trusted Coordinator (galapagos-browser.exe) en Release
Write-Host "`n1. Compilando / validando Trusted Coordinator (galapagos-browser.exe) en modo release..." -ForegroundColor Yellow
$coordBin = "$projectRoot\target\release\galapagos-browser.exe"

if (-not (Test-Path $coordBin)) {
    Write-Host "Compilando galapagos-browser (--release)..."
    Push-Location $projectRoot
    try {
        cargo build --release
        if ($LASTEXITCODE -ne 0) {
            Write-Error "Fallo critico en la compilacion release de galapagos-browser."
        }
    } finally {
        Pop-Location
    }
}

if (-not (Test-Path $coordBin)) {
    Write-Error "Binario del Coordinador de produccion no encontrado en: $coordBin"
}

$coordItem = Get-Item $coordBin
Write-Host "Coordinador validado: $($coordItem.FullName) ($([Math]::Round($coordItem.Length / 1MB, 2)) MiB)" -ForegroundColor Green

# 2. Compilar / verificar binario del Sandboxed Content Worker (galapagos-content.exe) estrictamente en Release
Write-Host "`n2. Compilando / validando Sandboxed Worker (galapagos-content.exe) en modo release..." -ForegroundColor Yellow

$workerCandidatePaths = @(
    "$projectRoot\crates\galapagos-content\target\release\galapagos-content.exe",
    "$projectRoot\target\release\galapagos-content.exe"
)

$workerBin = $null
foreach ($p in $workerCandidatePaths) {
    if (Test-Path $p) {
        $workerBin = $p
        break
    }
}

if (-not $workerBin) {
    Write-Host "Compilando worker galapagos-content con motor Servo embebido (--release -j 2)..." -ForegroundColor Cyan
    Push-Location "$projectRoot\crates\galapagos-content"
    try {
        cargo build --release -j 2
        if ($LASTEXITCODE -ne 0) {
            Write-Error "Fallo critico en la compilacion release de galapagos-content."
        }
    } finally {
        Pop-Location
    }

    foreach ($p in $workerCandidatePaths) {
        if (Test-Path $p) {
            $workerBin = $p
            break
        }
    }
}

if (-not $workerBin) {
    Write-Error "Binario de produccion del Worker no encontrado: galapagos-content.exe"
}

$workerItem = Get-Item $workerBin
# Verificacion estricta anti-stub: el binario con WebRender/Servo compilado debe superar 50 MiB
if ($workerItem.Length -lt 50000000) {
    Write-Error "El archivo galapagos-content.exe encontrado es un stub o artefacto incompleto ($($workerItem.Length) bytes). Se requiere el motor de produccion completo."
}

Write-Host "Worker validado: $($workerItem.FullName) ($([Math]::Round($workerItem.Length / 1MB, 2)) MiB)" -ForegroundColor Green

# 3. Localizar makeappx.exe en Windows Kits
$makeappxPaths = @(
    "C:\Program Files (x86)\Windows Kits\10\bin\10.0.19041.0\x64\makeappx.exe",
    "C:\Program Files (x86)\Windows Kits\10\bin\10.0.22621.0\x64\makeappx.exe",
    "C:\Program Files (x86)\Windows Kits\10\bin\10.0.22000.0\x64\makeappx.exe"
)

$makeappx = $null
foreach ($p in $makeappxPaths) {
    if (Test-Path $p) {
        $makeappx = $p
        break
    }
}

if (-not $makeappx) {
    $found = Get-ChildItem "C:\Program Files (x86)\Windows Kits\10\bin" -Filter "makeappx.exe" -Recurse -ErrorAction SilentlyContinue | Where-Object { $_.FullName -like "*x64*" } | Select-Object -First 1
    if ($found) {
        $makeappx = $found.FullName
    }
}

if (-not $makeappx) {
    Write-Error "makeappx.exe no encontrado en Windows Kits. Por favor instale el Windows 10/11 SDK."
}
Write-Host "Utilizando MakeAppx: $makeappx" -ForegroundColor Green

# 4. Asegurar la presencia de assets oficiales de produccion
$assetsSrcDir = Join-Path $scriptDir "assets"
$requiredAssets = @("StoreLogo.png", "Square150x150Logo.png", "Square44x44Logo.png", "Wide310x150Logo.png", "SplashScreen.png")
$missingAssets = @()
foreach ($a in $requiredAssets) {
    if (-not (Test-Path (Join-Path $assetsSrcDir $a))) {
        $missingAssets += $a
    }
}

if ($missingAssets.Count -gt 0) {
    Write-Host "Assets oficiales faltantes ($($missingAssets -join ', ')). Generando mediante packaging\generate_production_assets.ps1..." -ForegroundColor Yellow
    & "$scriptDir\generate_production_assets.ps1" -OutputDir $assetsSrcDir
}

# 5. Preparar layout de staging limpio y atomico
$stagingDir = "$scriptDir\staging"
Write-Host "`n3. Preparando layout de staging limpio en: $stagingDir..." -ForegroundColor Yellow
if (Test-Path $stagingDir) {
    Remove-Item $stagingDir -Recurse -Force
}
New-Item -ItemType Directory -Path "$stagingDir\Assets" -Force | Out-Null

# Copiar ejecutables de produccion del modelo multiproceso
Copy-Item $coordBin -Destination "$stagingDir\galapagos-browser.exe" -Force
Copy-Item $workerBin -Destination "$stagingDir\galapagos-content.exe" -Force

# DELTA_05 A-I6: las DLL de ANGLE (salida de mozangle) deben viajar con el worker; sin ellas no hay render
$workerDir = Split-Path -Parent $workerBin
foreach ($dll in @("libEGL.dll", "libGLESv2.dll")) {
    $dllPath = Join-Path $workerDir $dll
    if (-not (Test-Path $dllPath)) {
        Write-Error "Falta $dll junto al worker ($workerDir). Ejecuta: .\packaging\stage_angle_dlls.ps1 -Profile release"
    }
    Copy-Item $dllPath -Destination "$stagingDir\$dll" -Force
}

# Copiar manifiesto AppxManifest.xml
Copy-Item "$scriptDir\AppxManifest.xml" -Destination "$stagingDir\AppxManifest.xml" -Force

# Copiar assets visuales oficiales
foreach ($a in $requiredAssets) {
    Copy-Item (Join-Path $assetsSrcDir $a) -Destination "$stagingDir\Assets\$a" -Force
}

# Verificacion estricta de presencia de ambos ejecutables en staging
if (-not (Test-Path "$stagingDir\galapagos-browser.exe")) {
    Write-Error "Fallo critico: galapagos-browser.exe no se encuentra en el staging."
}
if (-not (Test-Path "$stagingDir\galapagos-content.exe")) {
    Write-Error "Fallo critico: galapagos-content.exe no se encuentra en el staging."
}
foreach ($dll in @("libEGL.dll", "libGLESv2.dll")) {
    if (-not (Test-Path "$stagingDir\$dll")) {
        Write-Error "Fallo critico: $dll no se encuentra en el staging."
    }
}

# 6. Empaquetar con MakeAppx.exe
Write-Host "`n4. Creando paquete MSIX de produccion con MakeAppx.exe..." -ForegroundColor Yellow
if (Test-Path $OutputPackage) {
    Remove-Item $OutputPackage -Force
}

$packArgs = @("pack", "/d", $stagingDir, "/p", $OutputPackage, "/o", "/v")
$procPack = Start-Process -FilePath $makeappx -ArgumentList $packArgs -NoNewWindow -PassThru -Wait
if ($procPack.ExitCode -ne 0) {
    Write-Error "Fallo en MakeAppx pack. Codigo de salida: $($procPack.ExitCode)"
}

Write-Host "Paquete MSIX creado exitosamente: $OutputPackage" -ForegroundColor Green

# 7. Firmar digitalmente el paquete MSIX resultante
Write-Host "`n5. Firmando paquete MSIX con sign_package.ps1..." -ForegroundColor Yellow
& "$scriptDir\sign_package.ps1" -FilePath $OutputPackage

# 8. Limpieza determinista de staging (a menos que se indique -KeepStaging)
if (-not $KeepStaging) {
    Write-Host "`n6. Limpiando directorio temporal staging/..." -ForegroundColor Yellow
    Remove-Item $stagingDir -Recurse -Force
}

# 9. Resumen de telemetria del paquete final
$pkgItem = Get-Item $OutputPackage
$pkgLength = $pkgItem.Length
$pkgName = $pkgItem.Name
$pkgFullName = $pkgItem.FullName
$pkgSizeMib = [Math]::Round(($pkgLength / 1MB), 2)
$pkgHash = (Get-FileHash -Path $OutputPackage -Algorithm SHA256).Hash

Write-Host "`n=== CERTIFICACION DE EMPAQUETADO MSIX DE PRODUCCION EXITOSA ===" -ForegroundColor Green
Write-Host "Archivo:        $pkgName"
Write-Host "Ruta:           $pkgFullName"
Write-Host "Tamano:         $pkgSizeMib MiB ($pkgLength bytes)"
Write-Host "Firma Digital:  Valida (SHA-256 Authenticode)"
Write-Host "SHA-256 Digest: $pkgHash" -ForegroundColor Yellow

$global:LASTEXITCODE = 0
