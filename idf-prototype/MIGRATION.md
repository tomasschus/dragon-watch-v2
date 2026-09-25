# Migración del reloj a ESP-IDF

Estado: port funcional compilable, aún sin validación en el reloj físico. El
firmware anterior sigue disponible en `src/main.rs` y usa `esp-hal`; el nuevo
firmware está en `idf-prototype/`.

## Objetivo

- Rust sobre ESP-IDF 5.3 para ESP32-S3.
- CPU a 80 MHz normalmente; una app puede solicitar 160 MHz solo mientras
  permanece abierta. El gestor central libera el bloqueo al cerrar la app.
- `light_sleep_enable = false`. El deep sleep se invoca explícitamente con la
  secuencia actual de apagado y despertar.

## Mapa del hardware actual

| Función | Conexión o configuración actual | Migración |
| --- | --- | --- |
| Botón y despertar `ext0` | GPIO0, activo en bajo | `esp_sleep_enable_ext0_wakeup(0, 0)` antes de `esp_deep_sleep_start()` |
| I²C RTC, PMU y táctil | I2C0, SDA 15, SCL 14, 400 kHz | `esp-idf-hal` I²C; conservar `src/drivers/*.rs` |
| Táctil | INT 38, RESET 9 | GPIO de ESP-IDF; conservar protocolo FT5x06 |
| AMOLED SH8601 | SPI2 QSPI 40 MHz; SCK 11, CS 12, D0 4, D1 5, D2 6, D3 7; RESET 8 | `esp_lcd_sh8601` oficial o adaptador QSPI para `ControllerInterface` |
| Framebuffer | 410 × 502 RGB888, PSRAM | habilitar PSRAM en `sdkconfig`; comprobar memoria y DMA |
| USB hora | USB Serial/JTAG, comando `TIME,YYYY-MM-DDTHH:MM:SS` | lector serie equivalente |

## Estado del portado

Implementado en `idf-prototype/`:

- ESP-IDF 5.3 con gestión dinámica de energía habilitada. Perfil base de 80 MHz
  y bloqueo de rendimiento de 160 MHz durante la ejecución de una app; el guard
  se libera al cerrarla.
- Adaptadores para I²C, RTC, PMU, táctil, USB Serial/JTAG, reloj, navegación y
  apagado explícito a deep sleep. La interfaz de apps existente se integra por
  módulos compartidos; una app nueva se registra con un icono y su descriptor,
  que especifica el perfil de energía.
- Driver SH8601 QSPI implementado sobre SPI de ESP-IDF, conservando la
  interfaz `embedded-graphics`. El protocolo se contrastó con el proyecto
  local `dragon-watch-idf` y el firmware C anterior.
- Tickless idle/light sleep automático desactivado para evitar alterar el
  comportamiento de deep sleep ya existente.
- Inicio más rápido tras deep sleep: se omite el test de memoria de los 8 MB de
  PSRAM (ya había pasado en el reloj); el primer reloj simple se dibuja apenas
  termina la inicialización del panel. El log de arranque pasó de ~892 ms a
  ~473 ms hasta `cpu_start` (unos 419 ms menos).
- Ping pong mantiene estática la cancha, actualiza solo las regiones de pelota
  y paletas, y usa un tick de 16 ms mientras está abierto.
- `cargo build --release --offline` de `idf-prototype/` verificado.

Pruebas físicas reportadas:

- Táctil: funciona.
- Apagado de pantalla: funciona.

Pendiente de probar en hardware antes de reemplazar el firmware habitual:

1. Comprobar imagen, colores, orientación y estabilidad del QSPI.
2. Probar hora, batería, toque breve/largo, botón, menú y sincronización USB.
3. Medir que la frecuencia pasa de 80 a 160 MHz al abrir una app y vuelve a
   80 MHz al salir; confirmar que el bloqueo no queda retenido en transiciones.
4. Verificar deep sleep, despertar por GPIO0 y reinicialización RTC IO.
5. Medir corriente de deep sleep y autonomía con batería de 400 mAh.

El driver de pantalla usa transferencias SPI por bloques y una copia DMA en
memoria interna; el framebuffer RGB888 reside en PSRAM. El rendimiento visual,
las asignaciones reales y el consumo deben medirse en el dispositivo.

## Comportamiento a conservar

- Reloj atenuado tras 5 s de inactividad; deep sleep tras 65 s salvo resumen.
- Botón lateral apaga y el siguiente pulso despierta al reloj.
- Toque breve abre menú de apps; toque largo abre resumen.
- Hora persistente en PCF85063A y sincronización por USB.

## Fuentes

- ESP-IDF Power Management: https://docs.espressif.com/projects/esp-idf/en/v5.3/esp32s3/api-reference/system/power_management.html
- ESP-IDF Sleep Modes: https://docs.espressif.com/projects/esp-idf/en/v5.3/esp32s3/api-reference/system/sleep_modes.html
- Driver SH8601 de Espressif: https://components.espressif.com/components/espressif/esp_lcd_sh8601
- Integración Rust con ESP-IDF: https://github.com/esp-rs/esp-idf
