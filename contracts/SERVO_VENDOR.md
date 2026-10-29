# Contrato de Vendor: Servo

**Estado:** REGISTRADO (2026-10-23)  
**Versión de Servo:** `0.7.0`  
**Ubicación:** `vendor/servo`  
**Referencia:** [DELTA_01_engine_worker.md](file:///d:/App/Browser-SaveNav/contracts/DELTA_01_engine_worker.md) (Decisión D2)

---

## 1. Integridad y Árbol de Archivos

| Métrica | Valor Determinista |
|---|---|
| **Versión** | `0.7.0` |
| **Archivos totales** | `2703` |
| **Directorios totales** | `321` |
| **Tamaño total en disco** | `45,750,256 bytes` (~43.63 MB) |
| **Git Tree Hash (write-tree)** | `4b825dc642cb6eb9a060e54bf8d69288fbee4904` |
| **Root Tree SHA-256 (manifest)** | `28a6b2a317c60d37b511c574b6a424a1fe48ed979dc2236b343304f73666b4c5` |

---

## 2. Requisitos Previos de Compilación en Host (Windows x86_64)

Auditoría de toolchain requerida por Servo en Windows (`.cargo/config.toml` y `build.rs`):

| Requisito | Estado | Ruta / Versión en Host |
|---|---|---|
| **Python 3** (`script_bindings` en `build.rs`) | VERIFICADO | `Python 3.14.6` (disponible en PATH) |
| **LLVM / Clang** (`lld-link.exe` exigido por Servo) | VERIFICADO | `C:\Program Files\LLVM\bin\lld-link.exe` (LLVM 22.1.8 / clang en PATH) |
| **MSVC / C++ Build Tools** (toolchain Rust Windows) | VERIFICADO | `C:\Program Files (x86)\Microsoft Visual Studio\18\BuildTools` (v18.7.11925.98) |
| **Rust Toolchain (Default Host)** | VERIFICADO | `rustc 1.98.1` (`x86_64-pc-windows-msvc`) |
| **Rust Toolchain Servo (`rust-toolchain.toml`)** | VERIFICADO | `1.97.1-x86_64-pc-windows-msvc` (activo automáticamente en `vendor/servo`) |
| **Componentes de Servo 1.97.1** | VERIFICADO | `clippy`, `llvm-tools`, `rustc-dev`, `rustfmt`, `rust-src` instalados |

