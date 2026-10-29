# packaging/create_dev_cert.ps1
# Generacion determinista de certificado para empaquetado y firma de codigo (Code Signing)
# Conforme a ZNVE v2.4.2 (Axiomas 1 y 2, SEC-03)

[Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSAvoidUsingPlainTextForPassword', '')]
[Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSAvoidUsingConvertToSecureStringWithPlainText', '')]
[Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSAvoidUsingWriteHost', '')]
[CmdletBinding()]
param (
    [string]$Publisher = "CN=GalapagosTechnologies, O=Galapagos Technologies Inc., C=US",
    [string]$CertPath = "",
    [string]$Password = "GalapagosSecure2026!"
)

$ErrorActionPreference = "Stop"

if (-not $CertPath) {
    $scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
    $CertPath = Join-Path $scriptDir "GalapagosCert.pfx"
}

Write-Host "=== Generador de Certificado de Firma de Codigo (ZNVE SEC-03) ===" -ForegroundColor Cyan
Write-Host "Publisher: $Publisher"

# 1. Crear certificado para firma de codigo
$secPassword = ConvertTo-SecureString -String $Password -Force -AsPlainText

Write-Host "Generando certificado en Cert:\CurrentUser\My..."
$cert = New-SelfSignedCertificate `
    -Type CodeSigningCert `
    -Subject $Publisher `
    -KeyUsage DigitalSignature `
    -FriendlyName "Galapagos Technologies Code Signing Certificate" `
    -CertStoreLocation "Cert:\CurrentUser\My" `
    -NotAfter (Get-Date).AddYears(3) `
    -HashAlgorithm "SHA256"

Write-Host "Certificado creado con huella digital: $($cert.Thumbprint)" -ForegroundColor Green

# 2. Exportar a archivo PFX para firma reproducible
Write-Host "Exportando clave privada a PFX: $CertPath"
Export-PfxCertificate -Cert $cert -FilePath $CertPath -Password $secPassword | Out-Null

# 3. Exportar certificado publico DER (.cer) para despliegue de confianza
$cerPath = [System.IO.Path]::ChangeExtension($CertPath, ".cer")
Write-Host "Exportando certificado publico a CER: $cerPath"
Export-Certificate -Cert $cert -FilePath $cerPath | Out-Null

# 4. Instalar en TrustedPeople del usuario actual
Write-Host "Instalando certificado en Cert:\CurrentUser\TrustedPeople..."
$storePeople = New-Object System.Security.Cryptography.X509Certificates.X509Store "TrustedPeople", "CurrentUser"
$storePeople.Open([System.Security.Cryptography.X509Certificates.OpenFlags]::ReadWrite)
$storePeople.Add($cert)
$storePeople.Close()

# 5. Intentar instalar en TrustedPeople de LocalMachine
try {
    Write-Host "Intentando instalar en Cert:\LocalMachine\TrustedPeople..."
    $storeLmPeople = New-Object System.Security.Cryptography.X509Certificates.X509Store "TrustedPeople", "LocalMachine"
    $storeLmPeople.Open([System.Security.Cryptography.X509Certificates.OpenFlags]::ReadWrite)
    $storeLmPeople.Add($cert)
    $storeLmPeople.Close()
    Write-Host "Certificado instalado en Cert:\LocalMachine\TrustedPeople (Confianza a nivel de maquina)." -ForegroundColor Green
} catch {
    Write-Host "Aviso: Escritura en Cert:\LocalMachine\TrustedPeople requiere privilegios de Administrador." -ForegroundColor Yellow
    Write-Host "Para que Windows App Installer GUI reconozca el paquete sin error 0x800B010A, ejecute como Admin:" -ForegroundColor Yellow
    Write-Host "  Import-Certificate -FilePath `"$cerPath`" -CertStoreLocation Cert:\LocalMachine\TrustedPeople" -ForegroundColor Cyan
}

# 6. Finalizacion
Write-Host "Certificado preparado exitosamente." -ForegroundColor Green
Write-Host "Listo para firmar binarios y paquetes MSIX."
