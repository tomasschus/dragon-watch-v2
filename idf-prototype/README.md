# Firmware ESP-IDF en migración

Este binario integra la pantalla SH8601 QSPI, RTC PCF85063A, PMU AXP2101,
control táctil, USB Serial/JTAG, la UI actual y perfiles de frecuencia. Falta
validar el comportamiento en el reloj físico.

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

El perfil mantiene 80 MHz sin una app activa y solicita 160 MHz mientras la app
con `PowerProfile::Performance` está abierta. `light_sleep_enable = false`
desactiva el light sleep automático. El botón configura `ext0` en GPIO0 y entra
en deep sleep después de apagar panel, táctil y rieles de alimentación.

La integración compiló en `release` con ESP-IDF 5.3. El `sdkconfig` generado
confirmó `CONFIG_PM_ENABLE=y`, ESP32-S3 y `CONFIG_FREERTOS_USE_TICKLESS_IDLE`
desactivado. Faltan pruebas físicas: imagen QSPI, toque, cambio real de MHz,
sincronización USB, despertar por GPIO0 y consumo en deep sleep.
