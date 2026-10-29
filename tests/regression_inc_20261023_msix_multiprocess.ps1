# tests/regression_inc_20261023_msix_multiprocess.ps1
# Test de regresion ZNVE para INC-20261023-MSIX-MULTIPROC-REFAC
# Valida el empaquetado y presencia determinista del Trusted Coordinator y Sandboxed Worker en el paquete MSIX

[Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSAvoidUsingWriteHost', '')]
[CmdletBinding()]
param ()

$ErrorActionPreference = "Stop"

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$projectRoot = Split-Path -Parent $scriptDir
$buildScript = Join-Path $projectRoot "packaging\build_msix.ps1"
$msixPath = Join-Path $projectRoot "packaging\GalapagosBrowser_0.1.0_x64.msix"
$stagingDir = Join-Path $projectRoot "packaging\staging"

Write-Host "=== Test de Regresion ZNVE: INC-20261023-MSIX-MULTIPROC-REFAC ===" -ForegroundColor Cyan

# 1. Ejecutar el orquestador de empaquetado en modo debug
Write-Host "Ejecutando build_msix.ps1 -Configuration debug..."
try {
    & $buildScript -Configuration debug
} catch {
    throw "FALLO: Error durante ejecucion de build_msix.ps1: $_"
}

# 2. Verificar layout de staging multiproceso
$coordPath = Join-Path $stagingDir "galapagos-browser.exe"
$workerPath = Join-Path $stagingDir "galapagos-content.exe"
$manifestPath = Join-Path $stagingDir "AppxManifest.xml"

if (-not (Test-Path $coordPath)) {
    throw "FALLO: galapagos-browser.exe no existe en staging."
}
if (-not (Test-Path $workerPath)) {
    throw "FALLO: galapagos-content.exe no existe en staging (regresion multiproceso detectada)."
}
if (-not (Test-Path $manifestPath)) {
    throw "FALLO: AppxManifest.xml no existe en staging."
}

$workerItem = Get-Item $workerPath
if ($workerItem.Length -lt 1000000) {
    throw "FALLO: galapagos-content.exe es un stub vacio o incompleto ($($workerItem.Length) bytes)."
}

Write-Host "Layout de staging multiproceso certificado: Coordinator y Worker presentes." -ForegroundColor Green

# 3. Verificar existencia e integridad del paquete MSIX generado
if (-not (Test-Path $msixPath)) {
    throw "FALLO: El paquete MSIX no fue generado en: $msixPath"
}

$msixItem = Get-Item $msixPath
if ($msixItem.Length -lt 10000000) {
    throw "FALLO: El paquete MSIX ($($msixItem.Length) bytes) no incluye los binarios completos del motor."
}

Write-Host "Paquete MSIX generado: $($msixItem.Name) ($([Math]::Round($msixItem.Length / 1MB, 2)) MiB)" -ForegroundColor Green

# 4. Validar firma digital Authenticode
$sig = Get-AuthenticodeSignature -FilePath $msixPath
if ($null -eq $sig.SignerCertificate) {
    throw "FALLO: El paquete MSIX no posee firma digital valida."
}

$expectedSubject = "CN=GalapagosBrowserDev, O=Galapagos, C=US"
if ($sig.SignerCertificate.Subject -ne $expectedSubject) {
    throw "FALLO: El firmante no coincide. Esperado: '$expectedSubject', Recibido: '$($sig.SignerCertificate.Subject)'"
}

Write-Host "Firma digital del paquete MSIX validada: $($sig.SignerCertificate.Thumbprint)" -ForegroundColor Green
Write-Host "TEST DE REGRESION INC-20261023-MSIX-MULTIPROC-REFAC: 100% EXITOSO" -ForegroundColor Green
