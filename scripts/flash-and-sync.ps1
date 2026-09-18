param([string]$Port = "COM4")

$ErrorActionPreference = "Stop"

# Siempre actualiza el RTC justo después de arrancar la imagen nueva.
cargo build --release
if ($LASTEXITCODE -ne 0) { throw "La compilacion fallo; no se flasheo el reloj." }
espflash flash --port $Port "target\xtensa-esp32s3-none-elf\release\resumen-watch"
if ($LASTEXITCODE -ne 0) { throw "El flash fallo; no se intento sincronizar el RTC." }
Start-Sleep -Milliseconds 800

$timestamp = (Get-Date).ToString("yyyy-MM-ddTHH:mm:ss")
$serial = [System.IO.Ports.SerialPort]::new($Port, 115200, 'None', 8, 'One')
try {
    $serial.Open()
    $serial.Write("TIME,$timestamp`n")
    Start-Sleep -Milliseconds 250
    Write-Output "RTC sincronizado: $timestamp"
}
finally {
    if ($serial.IsOpen) { $serial.Close() }
    $serial.Dispose()
}
