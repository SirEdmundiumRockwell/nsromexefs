# nsromexefs

Herramienta de línea de comandos que extrae **`exefs`** y **`romfs`** desde contenedores
**NSP** y **XCI** de Nintendo Switch — base, update y DLC — y fusiona los parches diferenciales
(BKTR) en un solo paso.

> **Estado:** v0.1 · **M0, M1, M2, M3 y M4 completados y validados.**
> `info`, `keys`, `dump` y `batch` funcionan con **NSP y XCI**, base, DLC y updates,
> incluida la fusión BKTR base+update en un solo paso con **3.842/3.842 archivos idénticos
> byte a byte** al árbol de referencia. `manifest.json` todavía no se escribe (M6).

---

## Qué hace

```console
nsromexefs dump "juego [v0].nsp" "juego [v1.1.0].nsp" -o salida -k prod.keys
```

```
salida/
  exefs/          main, main.npdm, rtld, sdk, subsdk0, ...
  romfs/          árbol completo (base + update fusionados)
  SHA256SUMS
  manifest.json   (M6 — todavía no se escribe)
```

- Lee **NSP** (PFS0) y **XCI** (HFS0, incluida su partición `update` interna).
- Detecta automáticamente base, update y DLC leyendo el **CNMT**, no el nombre del archivo,
  y acepta los contenedores en cualquier orden.
- Deriva las claves de contenido desde el **ticket** o desde **`title.keys`**.
- Fusiona los parches **BKTR** contra la base sin escribir NCA descifradas intermedias.
- Extrae una carpeta completa con `batch` en un layout `<app_id>/v<versión>`.
- Previsto: `verify` y manifest (M5-M6).

## Qué funciona hoy

**Extracción completa de un base o DLC:**

```console
nsromexefs dump "juego [v0].nsp" -o salida -k prod.keys -k title.keys
```

```
juego [v0].nsp
  título     01006BB00C6F0000 · v1.0.0 · meta v0 · base (Application)
  programa   6e94be1ee104bc78…  [clave OK (ticket)]
  exefs      6 archivos · 28.46 MB
  romfs      3836 archivos · 6.23 GB · 169 directorios
  salida     salida
  SHA256SUMS 3842 entradas
```

- Escribe `salida/exefs/` y `salida/romfs/` planos (o solo uno con `--exefs-only` /
  `--romfs-only`).
- `--dry-run` recorre el RomFS y muestra el plan **sin escribir nada**; detecta colisiones
  de nombres antes del primer byte.
- `--force` reemplaza una extracción anterior; sin él, se niega a sobrescribir.
- `--json` devuelve el mismo plan legible por máquina; `-v` lista cada archivo.
- Cada archivo se escribe en un `.part` y se renombra al terminar: una interrupción no deja
  archivos a medias con el nombre definitivo.
- `SHA256SUMS` en formato GNU (hash de 64 hex + dos espacios + ruta relativa), ordenado.

Validación sobre *Link's Awakening* v1.0.0 (NSP real, 6,27 GB): 3.836 archivos de RomFS y
6 de exefs, con `main` = 14.984.660 B. Comparado contra el árbol 1.1.0 extraído con
hactool/nstool, **3.835 de 3.836 archivos son idénticos byte a byte**; el único distinto es
`region_common/event/SinkingSword.bfevfl` (2.712 B en base, 2.920 B en el update), que es
precisamente el archivo que toca el parche.

**Extracción de base + update, con fusión BKTR:**

```console
nsromexefs dump "juego [v0].nsp" "juego [v1.1.0].nsp" -o salida -k prod.keys -k title.keys
```

```
juego [v0].nsp
juego [v1.1.0].nsp
  título     01006BB00C6F0800 · v1.1.0 · meta v131072 · update (Patch)
  base       01006BB00C6F0000 · v1.0.0 · meta v0
  programa   88d7f50b596192d4…  [clave OK (ticket)]
  exefs      6 archivos · 28.46 MB
  romfs      3836 archivos · 6.23 GB · 169 directorios · fusionado con la base
  salida     salida
  SHA256SUMS 3842 entradas
```

- El orden de los argumentos da igual: la base y el update se identifican por su **CNMT**,
  no por el tamaño ni el nombre, y se comprueba que el update pertenece a esa aplicación.
- `exefs` sale del update cuando lo trae (código parcheado); si no, de la base.
- La RomFS virtual del update se resuelve **bloque a bloque** contra la base: no se escribe
  ninguna NCA intermedia de 6 GB.
- Un update sin su base se rechaza con `E_BASE_MISSING` (código de salida 3); indicar dos
  contenedores base en la misma invocación también es un error.

Validación de la fusión sobre *Link's Awakening* 1.1.0 (base v0 + update v1.1.0 reales):
6 archivos de `exefs` (`main` = 14.983.439 B), 3.836 archivos de `romfs`
(6.232.003.903 B) y 169 directorios. Frente al árbol 1.1.0 de referencia,
**3.842 de 3.842 archivos son idénticos byte a byte (SHA-256), 0 distintos**; la extracción
completa tarda ~12 s.

**Cartuchos XCI, layout por título y modo lote (M4):**

```console
nsromexefs dump juego.xci --tree -o salida -k prod.keys -k title.keys
nsromexefs batch "carpeta de juegos" -o salida -k prod.keys -k title.keys
```

- Un **XCI** se abre por su cabecera `HEAD` y su HFS0 raíz: `secure` aporta el juego y, si
  existe, la partición `update` se fusiona automáticamente con la base en la misma pasada.
- Un cartucho **no lleva tickets**: la clave del programa debe venir de `title.keys`. Sin ella
  el error es `E_KEY_TITLEKEY_MISSING` (código de salida 2) con una pista explícita.
- `--tree` organiza la salida como `<app_id>/v<versión>` para base/update y
  `<app_id>/dlc/<dlc_id>/v<versión>` para DLC. La versión se sanea antes de usarla como
  nombre de directorio.
- `batch` recorre la carpeta de forma recursiva, agrupa por aplicación (base, update y DLC),
  elige la versión más alta de cada uno y extrae con layout de árbol. Ignora enlaces
  simbólicos y reporta los `.nsz`/`.xcz` como omitidos.
- Un título que falla **no aborta el lote**: se registra, se continúa y el código de salida
  resume el conjunto (`E_BATCH_FAILED`; 3 si todos los fallos son de base, 1 si son mixtos).

Validación de M4: `batch` sobre los NSP reales de *Link's Awakening* produjo 3.842/3.842
archivos idénticos a la referencia en 12,9 s. Para la ruta XCI no había un cartucho real
disponible: se construyó un **XCI sintético con las NCA reales** (particiones `secure` y
`update`, sin tickets) y se extrajo con `title.keys`, obteniendo también 3.842/3.842 archivos
idénticos en 11,5 s. El parser HFS0/XCI está cubierto con pruebas unitarias y con esa
validación de extremo a extremo, pero **aún no se ha probado con un volcado de cartucho
comercial**.

**Inspección sin extraer:**

```console
nsromexefs info juego.nsp -k prod.keys -k title.keys
```

```
juego.nsp
  NSP (PFS0) · 7 entradas · 39.61 MB
  título     01006BB00C6F0800 · v1.1.0 · meta v131072 · update (Patch) · base 01006BB00C6F0000
  claves     crypto_type 10 → revisión 9 · meta vía key area
  contenidos del CNMT
    programa     37.98 MB  88d7f50b596192d4…  [clave OK (ticket)] · PFS0 AES-CTR, RomFS BKTR (parche)
    control       1.48 MB  b0cd3ef4d0f1e0e1…  [clave OK (key area)] · RomFS AES-CTR
    legal       149.50 KB  a0a715603f713fe4…  [clave OK (key area)] · RomFS AES-CTR
    delta        28.64 MB  b9bf85b9f8b33c74…  (fuera del contenedor)
    ...
```

- Descifra la cabecera NCA (AES-XTS), el key area y los tickets.
- Encuentra el PFS0 dentro de cada NCA usando su superbloque (`pfs0_offset`), no por suposición.
- Lee el CNMT y lista los contenidos reales con su tipo, tamaño y estado de clave.
- Informa de los fragmentos delta que viven fuera del contenedor sin tratarlos como error.
- `--json` para integraciones; `keys` resume el keyset cargado (sin imprimir material).

## Qué **no** hace

- No incluye claves. Las aporta el usuario.
- No se integra con launchers ni recompiladores: escribe un árbol de archivos y termina.
- No instala nada en ninguna consola ni modifica otros programas.
- No soporta (v1) NSZ/NCZ comprimidos ni tickets personalizados.

## Estado del proyecto

| Fase | Estado |
|---|---|
| Diseño técnico | ✅ cerrado — [`DISENO-nsromexefs.md`](DISENO-nsromexefs.md) |
| M0 — CLI, claves, PFS0/NSP, `info` | ✅ validado sobre NSP reales |
| M1 — NCA (XTS), key area, tickets, CNMT | ✅ validado sobre NSP reales |
| M2 — `exefs` + `romfs` (IVFC) | ✅ validado contra árbol de referencia (3.835/3.836 idénticos) |
| M3 — fusión de parches BKTR | ✅ validado: 3.842/3.842 idénticos, 0 distintos |
| M4 — XCI/HFS0, lote, `--tree` | ✅ validado con XCI sintético y lote real (3.842/3.842) |
| M5 — `verify` y preflight | ⏳ pendiente |
| M6 — `manifest.json`, `--resume` | ⏳ pendiente |
| GUI (Tauri 2 + Svelte) | ⏳ v2 (~5-6 días) |

## Documentación

- [**Diseño técnico**](DISENO-nsromexefs.md) — arquitectura, formatos, algoritmos y plan.
- [Licencia](LICENSE) — MIT.
- [Créditos y licencias de terceros](THIRD-PARTY-NOTICES.md).

## Requisitos (versión final)

- Un volcado propio del juego (`.nsp` o `.xci`) y, si aplica, su update y DLC.
- Un archivo de claves obtenido de tu propia consola (`prod.keys` y, opcionalmente, `title.keys`).
- Nada más: el binario será autocontenido.

## Aviso legal

`nsromexefs` **no** distribuye claves, contenido de juegos ni herramientas de elusión.
No está afiliado a Nintendo. Uso previsto: recompilación, descompilación y preservación de juegos
que el usuario posee legalmente. La responsabilidad del uso recae en el usuario.

Las marcas mencionadas pertenecen a sus respectivos titulares.

## Licencia

MIT — ver [`LICENSE`](LICENSE).

Este proyecto reutiliza conocimiento y lógica de proyectos de código abierto; consulta
[`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md) para la atribución completa.
