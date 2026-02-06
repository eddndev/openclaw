# OpenClaw IPv6 Rotation & Anti-Ban Strategy

**Fecha:** 3 de Febrero de 2026
**Objetivo:** Evitar baneos de WhatsApp (y otros servicios) utilizando el vasto espacio de direcciones IPv6 para asignar una IP única a cada agente.

---

## 1. El Problema: Reputación de IP
WhatsApp monitorea agresivamente la reputación de las direcciones IP. Correr múltiples cuentas (ej. 100) desde una sola dirección IPv4 pública es un patrón claro de comportamiento "bot" o "sybil", lo que lleva a baneos masivos de números.

## 2. La Solución: IPv6 /64 Subnet
La mayoría de los servidores modernos (Hetzner, OVH, AWS, etc.) asignan un bloque `/64` de IPv6. Esto contiene **18,446,744,073,709,551,616 direcciones**.
Podemos asignar una IP única y estática a cada agente, haciendo que para WhatsApp parezcan venir de dispositivos/redes totalmente distintos.

---

## 3. Implementación Técnica

### A. Requisito de Infraestructura (Servidor)
El servidor debe estar configurado para permitir el binding a **cualquier** IP dentro de su subred asignada ("Non-local bind" o rutas específicas).

*   **Linux (Sysctl):** Habilitar "AnyIP" para bindear rangos enteros.
    ```bash
    ip -6 route add local 2001:db8:1234::/64 dev eth0
    ```
    *(Esto le dice al kernel que todo el rango le pertenece, permitiendo a los procesos hacer bind a IPs específicas sin añadirlas una por una a la interfaz).*

### B. Fleet Commander (Rust) - Lógica de Asignación
El gestor de flotas debe calcular deterministicamente la IPv6 basada en el ID del agente.

```rust
// Pseudo-código Rust
fn calculate_ipv6(subnet_prefix: &str, agent_id: u32) -> String {
    // Ejemplo: 2001:db8:: + 1 (agent 1) = 2001:db8::1
    // Convertir a hex y concatenar
    format!("{}{:x}", subnet_prefix, agent_id)
}

// Al lanzar el proceso:
command.env("OPENCLAW_BAILEYS_BIND_IP", agent_ipv6);
```

### C. Modificación de OpenClaw (Patching)
Actualmente, `src/web/session.ts` no lee esta configuración. Necesitamos aplicar un pequeño parche al código fuente para inyectar el `https.Agent` con la IP específica.

**Archivo a modificar:** `src/web/session.ts`

**Código a inyectar (Concepto):**

```typescript
import { Agent } from "https";

// ... dentro de createWaSocket ...

const bindIp = process.env.OPENCLAW_BAILEYS_BIND_IP;
let agent: Agent | undefined;

if (bindIp) {
  // Configurar el agente para usar la IP específica y forzar IPv6
  agent = new Agent({
    localAddress: bindIp,
    family: 6,
    keepAlive: true
  });
  logger.info(`Using specific bind IP: ${bindIp}`);
}

const sock = makeWASocket({
  // ... resto de config ...
  agent, // <--- INYECTAR AQUÍ
  // ...
});
```

---

## 4. Plan de Acción

1.  **Validar Subred:** Confirma que tu servidor tiene un bloque `/64` asignado y enrutado.
2.  **Configurar OS:** Ejecuta el comando `ip -6 route add local ...` en el servidor host.
3.  **Aplicar Parche:** ✅ **COMPLETADO** - Modificado `src/web/session.ts` para soportar `OPENCLAW_BAILEYS_BIND_IP`.
4.  **Actualizar Fleet Commander:** Añadir la lógica de generación de IPs y la variable de entorno.

Esta estrategia es el "Gold Standard" para operaciones de scraping y automatización a gran escala. Al combinarlo con la arquitectura de procesos nativos, obtienes un sistema extremadamente robusto y difícil de detectar.
