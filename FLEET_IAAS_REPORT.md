# OpenClaw Fleet IaaS: Informe de Viabilidad y Arquitectura

**Fecha:** 3 de Febrero de 2026
**Estado:** ✅ VALIDADO / LUZ VERDE
**Contexto:** Despliegue masivo de agentes OpenClaw (WhatsApp/Telegram) en infraestructura "Bare Metal" sin virtualización (Docker).

---

## 1. Resumen Ejecutivo

La investigación y la Prueba de Concepto (PoC) confirman que **es totalmente viable y altamente eficiente** construir un gestor de flotas (**Fleet Commander**) utilizando **Rust** para orquestar procesos nativos de OpenClaw.

La arquitectura propuesta elimina la necesidad de Docker, reduciendo drásticamente el consumo de RAM y CPU, permitiendo una densidad de cientos/miles de agentes por nodo, manteniendo un aislamiento estricto de datos y configuración.

## 2. Fundamentos Técnicos (El "Por Qué" Funciona)

El análisis del código fuente de OpenClaw reveló tres comportamientos arquitectónicos que permiten la multi-tenencia nativa mediante "Falsificación de Contexto":

### A. Aislamiento por Sistema de Archivos (`HOME`)
El agente resuelve sus rutas de escritura (sesiones, logs, auth) basándose en `os.homedir()`.
- **Hallazgo:** Al inyectar la variable de entorno `HOME=/var/fleets/f1/agent-X`, el proceso crea su propio universo `.openclaw` aislado en esa ruta. No toca el directorio del usuario real.

### B. Gestión Automática de Puertos (Port Shifting)
No es necesario configurar cada puerto individualmente. El sistema usa un puerto base y calcula los demás mediante desplazamientos (offsets).
- **Fuente:** `src/config/port-defaults.ts`
- **Lógica:**
    - Gateway: `BASE`
    - Bridge Server: `BASE + 1`
    - Browser Control: `BASE + 2`
    - Canvas Host: `BASE + 4`
    - Chrome CDP: `BASE + 11` (rango dinámico)
- **Implicación:** Asignando bloques de puertos (ej: saltos de 100), se garantiza cero colisiones internas.

### C. Bloqueo Inteligente (Smart Locking)
El sistema de prevención de doble ejecución no es global, sino contextual.
- **Fuente:** `src/infra/gateway-lock.ts`
- **Mecanismo:** El archivo `.lock` se genera en `/tmp/openclaw-<uid>/` pero su nombre incluye un **Hash SHA1 de la ruta de configuración**.
- **Resultado:** Dos agentes con configs en rutas distintas (`.../agent-A/config.json` vs `.../agent-B/config.json`) generan locks distintos y pueden coexistir.

---

## 3. Metodología de la Prueba de Concepto (PoC)

Para validar la teoría, se realizó una simulación manual en el entorno local:

1.  **Preparación:** Se crearon directorios aislados para dos agentes (`agent_alpha` y `agent_beta`).
2.  **Configuración:** Se inyectaron archivos `openclaw.json` mínimos con:
    - `mode: "local"`
    - `port: 20000` (Alpha) y `20100` (Beta)
    - `auth.token`: Obligatorio para modo local.
3.  **Ejecución:**
    ```bash
    env HOME=.../agent_alpha openclaw gateway run
    ```
4.  **Verificación:**
    - **Procesos:** Ambos corrieron simultáneamente.
    - **Red:** Alpha escuchó en 20000, Beta en 20100.
    - **Locks:** Se generaron dos archivos `.lock` distintos en `/tmp`.
    - **Logs:** Logs independientes generados correctamente.

---

## 4. Especificaciones del Fleet Commander (Rust)

El binario en Rust será el "padre" responsable de la vida y muerte de los agentes.

### Estructura de Datos
```rust
struct AgentConfig {
    id: String,
    fleet_id: String,
    base_port: u16,
    auth_token: String,
}
```

### Ciclo de Vida (Workflow)

1.  **Cálculo de Recursos:**
    - Determinar `HOME`: `/var/lib/fleets/{fleet_id}/{agent_id}`.
    - Asignar Puerto: `BASE_PORT_INICIAL + (agent_index * 100)`.
2.  **Provisionamiento (Setup):**
    - `fs::create_dir_all(agent_home)`.
    - Escribir `config.json` en `{agent_home}/.openclaw/openclaw.json`.
        - **IMPORTANTE:** Inyectar `gateway.auth.token`. Sin esto, el agente se niega a arrancar.
3.  **Lanzamiento (Spawn):**
    - Ejecutar el binario `openclaw` (o `node dist/index.js`) como proceso hijo (`Command::spawn`).
    - **Variables de Entorno Críticas:**
        - `HOME`: Ruta aislada.
        - `OPENCLAW_GATEWAY_PORT`: Puerto calculado.
        - `OPENCLAW_LOG_JSON`: `true` (Para facilitar la ingestión de logs por Rust).
        - `NODE_ENV`: `production`.
4.  **Supervisión (Watchdog):**
    - Monitorear el PID hijo.
    - Si el proceso termina con código != 0 (crash), esperar y reiniciar (Backoff exponencial).

---

## 5. Consideraciones Críticas y "Gotchas"

### A. Red y Canales
*   **WhatsApp / Telegram:** Funcionan por salida (Outbound/Polling). **No consumen puertos de entrada**. Son seguros para escalar masivamente.
*   **Webhooks (Opcional):** Si en el futuro se usan canales que requieren Webhooks (MS Teams, extensiones), el Fleet Commander deberá inyectar el puerto específico en el bloque `channels` del JSON.

### B. Gestión de Recursos
*   **Node.js Memory:** Cada proceso consumirá ~60-100MB base. Con 1000 agentes, necesitas ~64GB-100GB RAM disponibles.
*   **Chrome/Browser:** Si los agentes usan la herramienta `browser`, el consumo se disparará. Considerar limitar el uso de esta herramienta o usar un "Browser Pool" compartido si la RAM es crítica.
*   **Logs:** OpenClaw escribe logs a disco. Rust debe encargarse de rotarlos o limpiar carpetas antiguas para no llenar el disco, ya que no tendremos a Docker para limitar el tamaño de logs.

### C. Seguridad
*   **Auth Token:** Es mandatorio generar un token único por agente para el Gateway local.
*   **Binding:** Asegurar que `gateway.bind` sea `"loopback"` (127.0.0.1) para que los puertos no queden expuestos a Internet, solo accesibles internamente o vía VPN (Tailscale).

### D. Limpieza (Cleanup)
*   Si el Fleet Commander muere abruptamente (`SIGKILL`), los procesos hijos (agentes) podrían quedar huérfanos ("Zombies").
*   **Solución Rust:** Usar una librería de gestión de procesos que soporte "Process Groups" o manejar señales (`SIGTERM`/`SIGINT`) para matar a todos los hijos antes de salir.

## 6. Siguientes Pasos

1.  Inicializar proyecto Rust (`cargo new fleet-commander`).
2.  Implementar la lógica de creación de carpetas y JSON.
3.  Implementar el `Command::spawn` con las variables de entorno.
4.  Probar con 10 agentes simultáneos.
