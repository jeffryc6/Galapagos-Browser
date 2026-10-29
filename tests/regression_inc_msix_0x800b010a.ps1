# tests/regression_inc_msix_0x800b010a.ps1
# Test de regresion ZNVE para INC-20261022-MSIX-0x800B010A
# Valida la exportacion del certificado CER y su compatibilidad con los almacenes de confianza de Windows

[Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSAvoidUsingWriteHost', '')]
[CmdletBinding()]
param ()

$ErrorActionPreference = "Stop"

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$projectRoot = Split-Path -Parent $scriptDir
$cerPath = Join-Path $projectRoot "packaging\GalapagosDevCert.cer"
$pfxPath = Join-Path $projectRoot "packaging\GalapagosDevCert.pfx"

Write-Host "=== Test de Regresion ZNVE: INC-20261022-MSIX-0x800B010A ===" -ForegroundColor Cyan

# 1. Verificar existencia del archivo CER
if (-not (Test-Path $cerPath)) {
    throw "FALLO: El archivo de certificado publico $cerPath no existe."
}

# 2. Cargar certificado y verificar EKU Code Signing y Subject
$cert = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2 $cerPath
$expectedSubject = "CN=GalapagosBrowserDev, O=Galapagos, C=US"

if ($cert.Subject -ne $expectedSubject) {
    throw "FALLO: Subject no coincide. Esperado: '$expectedSubject', Recibido: '$($cert.Subject)'"
}

if ($cert.HasPrivateKey) {
    throw "FALLO: El certificado publico CER no debe contener clave privada."
}

# Verificar extension Code Signing (OID 1.3.6.1.5.5.7.3.3)
$hasCodeSigning = $false
foreach ($ext in $cert.Extensions) {
    if ($ext -is [System.Security.Cryptography.X509Certificates.X509EnhancedKeyUsageExtension]) {
        foreach ($oid in $ext.EnhancedKeyUsages) {
            if ($oid.Value -eq "1.3.6.1.5.5.7.3.3") {
                $hasCodeSigning = $true
            }
        }
    }
}

if (-not $hasCodeSigning) {
    throw "FALLO: El certificado no contiene el uso de clave mejorada (EKU) Code Signing."
}

Write-Host "Certificado CER verificado: $($cert.Thumbprint) | Subject: $($cert.Subject)" -ForegroundColor Green

# 3. Verificar sincronizacion con PFX
$pfxCert = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2 ($pfxPath, "GalapagosSecure2026!")
if ($pfxCert.Thumbprint -ne $cert.Thumbprint) {
    throw "FALLO: La huella digital del PFX ($($pfxCert.Thumbprint)) difiere de la del CER ($($cert.Thumbprint))."
}

# 4. Verificar presencia en TrustedPeople (CurrentUser o LocalMachine)
$inCurrentUser = $false
$store = New-Object System.Security.Cryptography.X509Certificates.X509Store "TrustedPeople", "CurrentUser"
$store.Open([System.Security.Cryptography.X509Certificates.OpenFlags]::ReadOnly)
foreach ($c in $store.Certificates) {
    if ($c.Thumbprint -eq $cert.Thumbprint) {
        $inCurrentUser = $true
        break
    }
}
$store.Close()

if (-not $inCurrentUser) {
    throw "FALLO: El certificado no se encuentra registrado en Cert:\CurrentUser\TrustedPeople."
}

Write-Host "Certificado verificado en Cert:\CurrentUser\TrustedPeople." -ForegroundColor Green
Write-Host "TEST DE REGRESION INC-20261022-MSIX-0x800B010A: 100% EXITOSO" -ForegroundColor Green
