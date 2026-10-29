# packaging/install_package.ps1
# Instalador determinista de paquete MSIX de Galapagos Browser v0.1
# Conforme a ZNVE v2.4.2 (Axiomas 1 y 2, INC-20261022-MSIX-0x800B010A)

[Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSAvoidUsingWriteHost', '')]
[CmdletBinding()]
param (
    [string]$PackagePath = "",
    [string]$CertPath = ""
)

$ErrorActionPreference = "Stop"

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
if (-not $PackagePath) {
    $PackagePath = Join-Path $scriptDir "GalapagosBrowser_0.1.0_x64.msix"
}
if (-not $CertPath) {
    $CertPath = Join-Path $scriptDir "GalapagosCert.cer"
}

Write-Host "=== Instalador Automatizado de Galapagos Browser MSIX ===" -ForegroundColor Cyan
Write-Host "Paquete:     $PackagePath"
Write-Host "Certificado: $CertPath"

if (-not (Test-Path $PackagePath)) {
    throw "No se encontro el paquete MSIX en: $PackagePath. Ejecute packaging/build_msix.ps1 primero."
}

$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)

# 1. Asegurar confianza del certificado
if (Test-Path $CertPath) {
    if ($isAdmin) {
        Write-Host "Importando certificado a Cert:\LocalMachine\TrustedPeople (Modo Administrador)..."
        Import-Certificate -FilePath $CertPath -CertStoreLocation "Cert:\LocalMachine\TrustedPeople" | Out-Null

        # Habilitar sideloading si es necesario
        $regPath = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\AppModelUnlock"
        if (Test-Path $regPath) {
            Set-ItemProperty -Path $regPath -Name "AllowAllTrustedApps" -Value 1 -Force -ErrorAction SilentlyContinue
        }
    } else {
        Write-Host "Importando certificado a Cert:\CurrentUser\TrustedPeople (Modo Usuario)..."
        Import-Certificate -FilePath $CertPath -CertStoreLocation "Cert:\CurrentUser\TrustedPeople" | Out-Null
    }
} else {
    Write-Host "Aviso: No se encontro el archivo .cer en $CertPath. Omitiendo importacion automatica." -ForegroundColor Yellow
}

# 2. Despliegue del paquete MSIX
Write-Host "Instalando paquete MSIX..." -ForegroundColor Cyan
try {
    Add-AppxPackage -Path $PackagePath
    Write-Host "Galapagos Browser v0.1 instalado exitosamente." -ForegroundColor Green
    Write-Host "Puede ejecutar la aplicacion desde el Menu Inicio o buscando 'Galapagos Browser'."
} catch {
    $err = $_
    Write-Host "Error durante el despliegue del paquete:" -ForegroundColor Red
    Write-Host $err.Exception.Message -ForegroundColor Red

    if (-not $isAdmin) {
        Write-Host "`nSi el fallo es de confianza (0x800B010A), ejecute este script como Administrador:" -ForegroundColor Yellow
        Write-Host "  Start-Process powershell -Verb RunAs -ArgumentList `"-ExecutionPolicy Bypass -File `"`"$($MyInvocation.MyCommand.Path)`"`"`"" -ForegroundColor Cyan
    }
    throw $err
}
