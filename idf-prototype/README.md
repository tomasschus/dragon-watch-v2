# Prototipo de migración a ESP-IDF

Este binario comprueba la gestión de frecuencia dinámica en el ESP32-S3. No es
todavía el firmware del reloj: no inicializa la pantalla ni entra en deep sleep.
No flashearlo para uso diario.

## Compilar

Usar la toolchain Rust `esp`, ESP-IDF 5.3 y las herramientas de ESP-IDF. Si el
SDK ya está instalado, definir `IDF_PATH` en el entorno antes de compilar.
En Windows, `esp-idf-sys` necesita un directorio de salida corto. Desde este
directorio, por ejemplo:

```powershell
$env:IDF_PATH = 'D:\esp-idf'
$env:CARGO_TARGET_DIR = 'D:\t'
$env:CARGO_WORKSPACE_DIR = (Get-Location).Path
cargo build --release --offline
```

Las rutas son ejemplos de esta máquina; deben ajustarse en otros equipos.

El perfil mantiene 80 MHz sin una app activa y solicita 160 MHz mientras
`PerformanceGuard` está vivo. `light_sleep_enable = false` impide que este
prototipo cambie de modo de sueño automáticamente. El deep sleep del firmware
actual no se modifica.

La compilación del 24/09/2026 terminó correctamente en `release` con ESP-IDF
5.3. El `sdkconfig` generado confirmó `CONFIG_PM_ENABLE=y`, ESP32-S3 y
`CONFIG_FREERTOS_USE_TICKLESS_IDLE` desactivado. Falta verificar los MHz y el
consumo con el reloj físico.
