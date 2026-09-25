# Resumen Watch

Firmware Rust `no_std` para el reloj ESP32-S3 AMOLED 410×502.

## Uso

- Botón lateral: apagado real (deep sleep); el siguiente pulso inicia siempre en el reloj.
- Toque breve: abre el menú de apps, vacío por ahora; otro toque vuelve al reloj.
- Mantener presionado: abre el resumen de fórmulas.
- Deslizamiento horizontal: cambia entre MRU, MRUV, movimiento circular y movimiento relativo. Los puntos inferiores indican la página activa.
- No hay apagado automático de pantalla.

El firmware no inicializa Wi-Fi, Bluetooth, IMU, audio ni otros sensores. CPU a 80 MHz, pantalla negra/blanca y brillo fijo al 40%.

## Poner hora desde PC

Con el reloj conectado por USB, abrir el monitor serie a 115200 y enviar:

```text
TIME,2026-09-13T14:30:00
```

La hora se guarda en el RTC y persiste al apagar el reloj.

## Compilar y flashear

```powershell
. $env:USERPROFILE\export-esp.ps1
.\scripts\flash-and-sync.ps1 -Port COM4
```

El script recompila, flashea y sincroniza automáticamente el RTC con la hora local de la PC.
