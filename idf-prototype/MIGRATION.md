# Migración del reloj a ESP-IDF

Estado: prototipo de gestión de frecuencia. El firmware de uso diario sigue en
`src/main.rs` y usa `esp-hal`. No se flasheó el prototipo.

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

## Orden de portado

1. Compilar este prototipo con `CONFIG_PM_ENABLE` y verificar que el bloqueo
   de frecuencia se adquiere y libera. Medir la frecuencia real en hardware.
2. Portar I²C, PMU, RTC y táctil; comprobar lectura de hora, batería y toque.
3. Portar pantalla QSPI. El driver `Ws18AmoledDriver` de `sh8601-rs` usa tipos
   concretos de `esp-hal`, así que debe reemplazarse; la interfaz de dibujo
   `ControllerInterface` y `embedded-graphics` pueden conservarse.
4. Portar eventos, temporizadores y USB. Mantener navegación y apps actuales.
5. Reproducir exactamente la secuencia de `turn_off`: brillo cero, display off,
   panel sleep, táctil sleep, alimentación de pantalla off, RESET táctil bajo,
   despertar `ext0` por GPIO0 bajo, deep sleep. Comprobar el estado de GPIO0 al
   despertar, pues ESP-IDF deja el pin configurado como RTC IO.
6. Medir consumo en deep sleep y autonomía frente al firmware actual antes de
   sustituir el binario principal.

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
