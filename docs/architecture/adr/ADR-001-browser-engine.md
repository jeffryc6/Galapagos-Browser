# ADR-001: Selección de Servo como Motor Web Principal

* **Estado:** Aceptado
* **Fecha:** 2026-10-21
* **Responsable:** Director de Arquitectura
* **Contrato:** [contracts/GALAPAGOS — Arquitectura integral bajo Zero-Noise Vibe Engineering (ZNVE).md](file:///d:/App/Browser-SaveNav/contracts/GALAPAGOS%20%E2%80%94%20Arquitectura%20integral%20bajo%20Zero-Noise%20Vibe%20Engineering%20%28ZNVE%29.md)

## Contexto
El navegador requiere un motor de renderizado moderno para DOM, CSS, layout y JavaScript que respete los principios de mínima huella, seguridad de memoria y ausencia de telemetría invasiva.

## Decisión
Se adopta **Servo** (`libservo`, MPL-2.0) integrado vía bindings FFI en Rust.

## Alternativas Consideradas
1. **Chromium / CEF:** Descartado por el peso excesivo del binario (>150 MB), telemetría embebida difícil de erradicar y consumo desproporcionado de memoria en reposo.
2. **WebView2 (Edge):** Descartado por dependencia del sistema operativo y telemetría de Microsoft sin control total sobre las fronteras de red y cookies.
3. **Motor propio desde cero:** Descartado por inviabilidad técnica y mantenimiento de estándares web.

## Consecuencias
* **Positivas:** Seguridad de memoria nativa en Rust, arquitectura multihilo para layout/render, control total sobre el bucle de eventos.
* **Desafíos:** Madurez variable en ciertas APIs web emergentes y necesidad de verificación continua de la API embebida en Windows.
