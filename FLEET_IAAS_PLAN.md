# OpenClaw Fleet IaaS: Native Process Architecture Plan

## 1. El Objetivo
Crear una infraestructura **Multi-Tenant (Multi-Flota)** escalable para desplegar miles de agentes OpenClaw sin la sobrecarga de virtualización de Docker.
- **Eficiencia:** Usar procesos nativos de Node.js (Process Isolation) en lugar de contenedores.
- **Densidad:** Permitir cientos de agentes por servidor (limitado solo por RAM/CPU, no por overhead del runtime).
- **Gestión:** Un "Fleet Commander" (Rust) que orqueste ciclos de vida, puertos y contextos.

## 2. El Problema Actual (Docker)
- Docker añade latencia de arranque y consumo base de RAM por contenedor.
- La gestión de puertos estática en `docker-compose.yml` no escala.
- La inyección de archivos mediante volúmenes es rígida.

## 3. Hipótesis de Solución: "Falsificación de Contexto"
Creemos que OpenClaw puede ser engañado para correr múltiples instancias aisladas en la misma máquina manipulando su entorno:
- **Variable `HOME`:** Si `openclaw` basa su configuración en `~/.openclaw`, podemos lanzar cada proceso con `HOME=/var/lib/fleets/fleet-X/agent-Y`.
- **Variables de Entorno:** `OPENCLAW_CONFIG_PATH`, `OPENCLAW_PORT`, `OPENCLAW_WORKSPACE`.

## 4. Misión de Investigación (En código fuente)
Al analizar el repositorio `openclaw` (Typescript/Rust), debemos responder:
1.  **Resolución de Configuración:** ¿Qué archivo exacto controla dónde se buscan `config.json` y el `workspace`? (Buscar: `Paths`, `ConfigService`, `homedir`).
2.  **Singleton o Instanciable:** ¿El código asume que es el único proceso corriendo? (Ej: Bloqueo de archivos `.lock`, uso de puertos fijos hardcodeados).
3.  **Gateway Server:** ¿Cómo se define el puerto del servidor HTTP/WebSocket? ¿Acepta flags `--port` o ENV `PORT`?

## 5. Arquitectura Propuesta (Draft)
- **Filesystem Jerárquico:**
  ```text
  /var/lib/fleet-commander/
  ├── fleet-001/
  │   ├── global-knowledge/ (Enlace simbólico o compartido)
  │   ├── agent-01/ (.openclaw/config, workspace/)
  │   └── agent-02/
  ```
- **Execution:**
  `env HOME=/path/to/agent-01 PORT=3001 openclaw gateway run`

## 6. Siguientes Pasos
1.  Analizar el código fuente en este repositorio.
2.  Identificar las clases de configuración (`ConfigLoader` o similar).
3.  Probar manualmente el lanzamiento de dos agentes paralelos en puertos distintos sin Docker.
