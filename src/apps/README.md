# Apps del reloj

El menú usa `src/apps/mod.rs` como registro estático. Cada app vive en su propio archivo y exporta un descriptor con nombre, icono, apertura, manejo táctil y actualización periódica. El loop principal entrega eventos `Down`, `Move` y `Up` y llama `tick` cada 16 ms en ESP-IDF (50 ms en el driver anterior). El estado de la instancia se reinicia al abrirla y se entrega como `AppState`; no hacen falta tareas, variables globales mutables ni asignaciones por evento.

Para agregar una app:

1. Crear `src/apps/nombre.rs` y exportar `pub(crate) const APP: AppDescriptor`.
2. Implementar allí `draw_icon`, `open`, `touch` y `tick` (sin animación: `tick` devuelve `AppAction::Stay`).
3. Elegir `PowerProfile::Balanced` o `PowerProfile::Performance` en el descriptor. El perfil Performance eleva el CPU solo mientras la app está abierta.
4. Declarar `mod nombre;` y agregar `nombre::APP` al slice `APPS` de `mod.rs`.

El menú pagina nueve iconos por pantalla; no reserva memoria según el número de apps. Apps registradas: `flashlight.rs` (pantalla blanca a brillo máximo, un toque vuelve al menú) y `ping_pong.rs` (adaptado del Pong de `dragon-watch-rs`: saque con pausa breve, rival que ajusta su velocidad al marcador, juego a siete, toque en la franja superior para pausar y deslizar a la derecha estando pausado para salir). La cancha y la red se restauran dentro de cada zona sucia antes de transferirla al panel, con columnas alineadas para el SH8601. Ping pong usa `PowerProfile::Performance`; linterna mantiene el perfil base. `formulas.rs` sigue siendo un módulo independiente, accesible con pulsación larga.
