# GUI de nsromexefs

Aplicación de escritorio en Tauri 2 + Svelte 5. Este primer corte permite seleccionar uno o dos
NSP/XCI, validar claves, revisar el plan de `--dry-run`, extraer con opciones de árbol/reanudación
y ver los eventos y el resultado. La GUI delega las operaciones al ejecutable CLI; no contiene
lógica independiente de parsing ni extracción.

## Requisitos de desarrollo

- Node.js 22 y npm.
- Rust 1.90 o posterior para Tauri (MSVC en Windows). El motor CLI/core conserva MSRV 1.82.
- En Windows: Visual Studio Build Tools con C++ y WebView2 Runtime.
- En Linux: dependencias Tauri/WebKitGTK documentadas en la guía oficial de Tauri 2.

## Ejecutar en desarrollo

Desde esta carpeta:

```console
npm ci
npm run check
npm run tauri -- dev
```

La GUI intenta localizar `nsromexefs` junto a su ejecutable o en los directorios `target/debug`
y `target/release` de los directorios superiores. Si no aparece, abre **Ajustes** y selecciona el
binario CLI compilado con `cargo build --release -p nsromexefs-cli`.

Para compilar el frontend sin abrir la ventana:

```console
npm run build
```

## Estado

La GUI está en desarrollo y aún no forma parte de los paquetes de la release v0.1.0. El primer
corte no incluye extracción por lotes, selección de idioma, preferencias de tema ni instaladores
que empaqueten automáticamente GUI + CLI.
