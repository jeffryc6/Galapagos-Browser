# Packaging & Secure Distribution — Galapagos Browser v0.1

**Estado:** Fase F8 · Empaquetado & Distribucion Segura MSIX/Win32  
**Estandar de Gobernanza:** Zero-Noise Vibe Engineering (ZNVE v2.4.2)  
**Arquitectura:** Trusted Coordinator + Isolated AppContainer Workers (ARC-01, SEC-01, SEC-03)

---

## 1. Manifiesto de Aplicacion (`AppxManifest.xml`)

El empaquetado MSIX de Galapagos Browser establece fronteras de seguridad a nivel de sistema operativo:

* **Privilegios Restringidos (`runFullTrust`):**
  Declarado mediante el namespace `http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities`.
  * **Asignado unicamente a:** `galapagos-browser.exe` (Trusted Coordinator).
  * **Proposito:** Creacion y enjaulamiento de Windows Job Objects, configuracion de mitigaciones de proceso (ACG, MicrosoftSignedOnly) y administracion de ventanas Win32 nativas.
* **Aislamiento AppContainer:**
  Los procesos worker de renderizado (Servo) no poseen `runFullTrust`. Se ejecutan en un espacio de aislamiento AppContainer con:
  * Acceso exclusivo a red de loopback local para comunicacion Named Pipes con el Coordinador.
  * Acceso denegado a credenciales del usuario, archivos del sistema operativo o perifericos sin autorizacion explicita.

---

## 2. Herramientas y Pipeline de Empaquetado

| Archivo | Funcion |
|---|---|
| `AppxManifest.xml` | Manifiesto formal MSIX con capacidades declaradas, publisher e identidad corporativa. |
| `assets/` | Recursos graficos oficiales de alta fidelidad (StoreLogo, Square150x150Logo, Square44x44Logo, Wide310x150Logo, SplashScreen). |
| `generate_production_assets.ps1` | Generador determinista de assets de produccion con renderizado antialias de alta fidelidad. |
| `create_dev_cert.ps1` | Generacion de certificados con EKU `CodeSigning` para la identidad formal `CN=GalapagosTechnologies`. |
| `sign_package.ps1` | Firma automatizada de paquetes `.msix` mediante `signtool.exe` con digest SHA-256 y timestamp RFC 3161. |
| `build_msix.ps1` | Orquestador integral de produccion: validacion estricta de binarios release, layout staging, empaquetado MakeAppx, firma digital y limpieza. |
| `install_package.ps1` | Instalacion automatizada verificando la confianza del certificado en `TrustedPeople` antes del despliegue. |

---

## 3. Procedimiento de Construccion de Produccion

Ejecutar desde PowerShell:
```powershell
powershell -ExecutionPolicy Bypass -File .\packaging\build_msix.ps1 -Configuration release
```

El script ejecuta automaticamente los siguientes pasos sin mocks ni dependencias de debug:
1. **Compilacion/Validacion Release del Coordinador:** `target/release/galapagos-browser.exe`.
2. **Compilacion/Validacion Release del Worker:** `crates/galapagos-content/target/release/galapagos-content.exe` (con umbral minimo > 50 MiB garantizando la inclusion completa de Servo y WebRender).
3. **Copia de Assets de Produccion:** Copia desde `packaging/assets/` a `staging/Assets/`.
4. **Empaquetado Nativo:** `makeappx.exe pack /d packaging/staging /p packaging/GalapagosBrowser_0.1.0_x64.msix /o /v`.
5. **Firma Digital Authenticode:** `signtool.exe sign /f packaging/GalapagosCert.pfx /fd SHA256 /v packaging/GalapagosBrowser_0.1.0_x64.msix`.
6. **Limpieza Determinista:** Eliminacion automatica de `packaging/staging/`.

---

## 4. Instalacion del Paquete en Windows 10/11

### Metodo 1: Instalador Automatizado (Recomendado)
```powershell
powershell -ExecutionPolicy Bypass -File .\packaging\install_package.ps1
```

### Metodo 2: Instalacion Directa con PowerShell
```powershell
Add-AppxPackage -Path .\packaging\GalapagosBrowser_0.1.0_x64.msix
```

### Metodo 3: Instalador Grafico de Windows (Doble Clic)
Para registrar la confianza del certificado en la maquina local:
```powershell
# En PowerShell como Administrador:
Import-Certificate -FilePath .\packaging\GalapagosCert.cer -CertStoreLocation Cert:\LocalMachine\TrustedPeople
```
