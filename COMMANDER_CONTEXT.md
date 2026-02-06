# OpenClaw Commander: Contexto y Estado del Proyecto

**Fecha de guardado:** 06 de Febrero de 2026
**Objetivo:** Orquestador IaaS Multi-Tenant para flotas de agentes OpenClaw.

## 🧠 Arquitectura Actual

El **Commander** es un binario en Rust (`commander/`) que orquesta procesos nativos de Node.js (`openclaw.mjs`).

- **Modelo de Procesos:** No usa Docker por agente. Usa aislamiento lógico (directorios `HOME` únicos) y de red.
- **Networking:**
  - **API Commander:** Puerto `19999` (HTTP JSON).
  - **Agentes:** Puertos base `20000` + offset (`20100`, `20200`...).
  - **IPv6:** Cálculo matemático determinista (Prefijo + Índice) inyectado vía `OPENCLAW_BAILEYS_BIND_IP`.
- **Estado:** En memoria (`FleetState` Arc/Mutex). Persistencia vía sistema de archivos.

## 🛠️ Comandos Clave (Rust)

Desde la raíz del proyecto (`openclaw/`):

### 1. Iniciar la Flota

Levanta los agentes definidos, inyectando secretos y configuración.

```bash
cargo run --manifest-path commander/Cargo.toml -- start-fleet --count 3
```

### 2. Administración Interactiva ("Túnel")

Ejecuta comandos CLI de OpenClaw dentro del contexto aislado de un agente específico. Vital para **Login con QR (WhatsApp)** o **OAuth (Gemini)**.

```bash
# Login de WhatsApp
cargo run --manifest-path commander/Cargo.toml -- exec --id fleet-local-0 -- channels login --channel whatsapp

# Login de Gemini (OAuth)
cargo run --manifest-path commander/Cargo.toml -- exec --id fleet-local-0 -- models auth login --provider google-gemini-cli
```

### 3. API de Estado

```bash
curl http://localhost:19999/status
```

## ⚙️ Aprovisionamiento Automático (`ensure_config`)

El Commander genera automáticamente el entorno del agente en `.fleets/{id}/` si no existe.

**Descubrimientos Críticos Implementados:**

1.  **Plugins (Rutas):** Al cambiar el `HOME` del agente, OpenClaw pierde de vista las extensiones globales.
    - _Solución:_ El Commander inyecta rutas absolutas en `plugins.load.paths` apuntando a `openclaw/extensions/...`.
2.  **Aislamiento de Sesión:**
    - _Config:_ `session.dmScope = "per-channel-peer"`. Evita que el agente mezcle contextos de diferentes usuarios de WhatsApp.
3.  **Seguridad:**
    - _Permisos:_ Las carpetas `.openclaw` se crean con `chmod 700` (solo lectura para el dueño) para evitar advertencias de seguridad y proteger credenciales.
4.  **Secretos:**
    - El Commander lee el `.env` de la raíz del proyecto y lo hereda a los procesos hijos.

## 📝 Lista de Tareas Pendientes (Roadmap)

1.  **Watchdog / Supervisión:** Actualmente si un agente crashea, muere. Falta implementar una lógica de reinicio automático en Rust.
2.  **Dashboard UI:** Se implementó una API JSON básica. Falta revivir el Dashboard HTML (o hacer uno en React/Svelte) para visualizar la flota cómodamente.
3.  **Gestión de Ciclo de Vida:** Endpoints API para `POST /stop`, `POST /restart` de agentes individuales.
4.  **Logging Centralizado:** Agregar un mecanismo para ver los logs de todos los agentes en una sola terminal o archivo.

## 📂 Estructura de Directorios Generada

```text
openclaw/
├── .env                  # Secretos globales (API Keys)
├── commander/            # Código fuente Rust
├── extensions/           # Plugins (fuente real)
└── .fleets/              # Estado de la flota (gitignored)
    └── fleet-local-0/    # HOME aislado del agente 0
        └── .openclaw/    # Configuración y Credenciales
            ├── openclaw.json  # Generado por Commander
            └── auth/          # Sesiones (WhatsApp, Gemini)
```
