# PRIVACY AND RETENTION — Galapagos Browser v0.1

**Estado:** Política Formal de Privacidad y Retención de Datos (Fase C · Tarea 11)  
**Estándar de Gobernanza:** Zero-Noise Vibe Engineering (ZNVE v2.4.0)  
**Contrato Maestro:** [contracts/GALAPAGOS — Arquitectura integral bajo Zero-Noise Vibe Engineering (ZNVE).md](file:///d:/App/Browser-SaveNav/contracts/GALAPAGOS%20%E2%80%94%20Arquitectura%20integral%20bajo%20Zero-Noise%20Vibe%20Engineering%20%28ZNVE%29.md)  
**Entidades Asociadas:** [docs/contracts/DOMAIN_MODEL.md](file:///d:/App/Browser-SaveNav/docs/contracts/DOMAIN_MODEL.md)  

---

## 1. Principios Fundacionales de Privacidad (ZNVE-11)

1. **Privacidad Diagnóstica:** Se prohíbe el registro rutinario de URLs completas con cadenas de consulta (`query params`), contenido de formularios, contraseñas, tokens de autenticación o fragmentos de páginas.
2. **Telemetría Local por Excepción:** No existe envío de telemetría a servidores remotos ni recopilación ciega de actividad. Las muestras de rendimiento (`perf_baseline_samples`) se almacenan localmente y sólo registran magnitudes numéricas (bytes de RAM, microsegundos de CPU y latencia en milisegundos).
3. **Cero Retención Oculta:** La eliminación de datos de un perfil es exhaustiva e inmediata en los componentes que controla el navegador.

---

## 2. Matriz de Tratamiento de Datos por Tipo de Compartimento

| Categoría de Datos | Perfil Personal (`Personal` / `Daily`) | Perfil Efímero (`TemporaryPrivate` / `Ephemeral`) | Perfil Laboratorio (`Laboratory` / `Hardened`) |
|---|---|---|---|
| **Caché Web y Cookies 1P** | Persiste en disco cifrado con DPAPI / permisos de usuario. | En memoria RAM exclusivamente (`:memory:`); eliminada al cerrar la sesión. | Particionada en RAM por sesión de diagnóstico. |
| **Cookies de Terceros (3P)** | Bloqueadas por defecto (`Anti-Tracking Shield`). | Bloqueadas por defecto; almacenamiento particionado volátil. | Estrictamente bloqueadas; rechazo total de storage cruzado. |
| **Historial de Navegación** | Almacena host y timestamp normalizado. | **Prohibido:** cero registros en disco o memoria persistente. | Buffer temporal acotado a la duración de la sesión diagnóstica. |
| **Permisos de Sitio** | Persistentes en SQLite (`site_permissions`). | Temporales: válidos únicamente mientras la ventana esté abierta. | Modo solo-lectura o mock de permisos para pruebas. |
| **Telemetría de Rendimiento** | Muestras agregadas en `perf_baseline_samples` (máx. 1000 muestras por pestaña). | En memoria volátil; descartadas al destruir el perfil. | Alta resolución almacenada en búfer de diagnóstico acotado. |
| **Búferes de Renderizado** | Asignados por pestaña; liberados en `terminate_tab`. | Liberados y sobrescritos en memoria al cerrar la pestaña. | Sujetos a inspección por sondas de Research Island. |

---

## 3. Protocolo de Purga y Eliminación Verificable (PRIV-01 / PRIV-02)

Al invocar `UiToCoordinatorCmd::DestroyEphemeralProfile` sobre un perfil efímero (`TemporaryPrivate`), el Coordinador ejecuta la siguiente secuencia determinista:

```
[Orden de Destrucción de Perfil Efímero]
                  │
                  ▼
   1. Cierre síncrono de todas las pestañas asociadas
      (terminate_tab en cada TabId vinculado)
                  │
                  ▼
   2. Terminación forzosa del proceso worker de Servo
      (Desasignación del Job Object y cierre de handles)
                  │
                  ▼
   3. Purga y desasignación de la base de datos SQLite :memory:
      (drop de la conexión, liberando las páginas de RAM)
                  │
                  ▼
   4. Eliminación del ProfileId del registro del Coordinador
      (self.profiles.remove(&profile_id))
                  │
                  ▼
   Resultado: 0 archivos en disco, 0 páginas residuales en RAM
```

---

## 4. Política de Retención y Límites de Almacenamiento

1. **Línea Base de Rendimiento (`perf_baseline_samples`):**
   * Retención máxima: 30 días o un límite estricto de 10 000 registros totales por instalación.
   * Proyecciones de limpieza automática periódica ejecutada al arranque del navegador.
2. **Listas de Amenazas Verificadas (`verified_artifacts`):**
   * Se conservan únicamente las versiones activas de las listas; las versiones superadas se eliminan tras verificar la nueva firma criptográfica.
3. **Logs de Fallos y Excepciones:**
   * Almacenados en disco local con una cuota máxima de 10 MiB, rotados en modo FIFO y libres de cualquier identificador personal o URL confidencial.
