# nsromexefs

Herramienta de línea de comandos que extrae **`exefs`** y **`romfs`** desde contenedores
**NSP** y **XCI** de Nintendo Switch — base, update y DLC — y fusiona los parches diferenciales
(BKTR) en un solo paso.

> **Estado:** v0.1 · **M0–M6 completados y validados.**
> `info`, `keys`, `dump` y `batch` funcionan con **NSP y XCI**, base, DLC y updates,
> incluida la fusión BKTR base+update en un solo paso con **3.842/3.842 archivos idénticos
> byte a byte** al árbol de referencia. Incluye `verify`, preflight, `--version`, errores JSON,
> `manifest.json` y reanudación validada por hashes con `--resume`.

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
  manifest.json
```

- Lee **NSP** (PFS0) y **XCI** (HFS0, incluida su partición `update` interna).
- Detecta automáticamente base, update y DLC leyendo el **CNMT**, no el nombre del archivo,
  y acepta los contenedores en cualquier orden.
- Deriva las claves de contenido desde el **ticket** o desde **`title.keys`**.
- Fusiona los parches **BKTR** contra la base sin escribir NCA descifradas intermedias.
- Extrae una carpeta completa con `batch` en un layout `<app_id>/v<versión>`.
- `verify` relee `SHA256SUMS`; `--strict` también falla ante archivos sobrantes y un `main` sin `NSO0`.
- `--version` acepta versión visible, versión CNMT o entero interno. `--tree` extrae todos los
  títulos base/DLC de un contenedor combinado a carpetas separadas.
- Preflight: destino y espacio libre (datos + 1 % + 64 MiB) antes de empezar a escribir.
- `manifest.json` registra metadatos sin incluir secretos; `--resume` valida el plan, tamaño y
  SHA-256 de cada archivo antes de omitirlo. `verify` valida también el manifest cuando existe.

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
- Un mismo contenedor puede llevar **varios CNMT**: es habitual que un XCI "con update"
  guarde la base y el update dentro de `secure` (la partición `update` puede estar vacía).
  `dump`, `info` y `batch` leen todos los CNMT, no solo el primero.
- Los cartuchos no suelen incluir tickets. Si una NCA usa rights id y no hay ticket, la clave
  debe venir de `title.keys`; el error es `E_KEY_TITLEKEY_MISSING` (código de salida 2). Las
  NCA de un cartucho genuino suelen usar **key area**, así que `prod.keys` basta.
- `--tree` organiza la salida como `<app_id>/v<versión>` para base/update y
  `<app_id>/dlc/<dlc_id>/v<versión>` para DLC. La versión se sanea antes de usarla como
  nombre de directorio.
- `batch` recorre la carpeta de forma recursiva, agrupa por aplicación (base, update y DLC),
  elige la versión más alta de cada uno y extrae con layout de árbol. Ignora enlaces
  simbólicos y reporta los `.nsz`/`.xcz` como omitidos.
- Un título que falla **no aborta el lote**: se registra, se continúa y el código de salida
  resume el conjunto (`E_BATCH_FAILED`; 3 si todos los fallos son de base, 1 si son mixtos).

Validación de M4 con **XCI real** (*Link's Awakening* 1.1.0, 6,31 GB): el cartucho trae 11
entradas en `secure` con **dos CNMT** (base v1.0.0 y update v1.1.0) y la partición `update`
vacía. `dump --tree` y `batch` detectaron ambos títulos, fusionaron el update y produjeron
**3.842/3.842 archivos idénticos a la referencia en ~12 s usando solo `prod.keys`** (la base
va por key area y el update trae su ticket). Esa prueba descubrió y corrigió un fallo real:
antes solo se leía el primer CNMT del contenedor. El XCI sintético anterior sigue cubriendo
el caso de un cartucho recortado sin tickets, que exige `title.keys`.

**Verificación y selección de versión (M5):**

```console
nsromexefs verify salida --strict
nsromexefs dump juego.xci --tree --version 1.0.0 -o salida -k prod.keys
nsromexefs dump juego.xci --tree -o salida --resume -k prod.keys
```

`verify` comprueba cada SHA-256 de `SHA256SUMS`, informa archivos faltantes, dañados, inválidos
y sobrantes; no sigue enlaces simbólicos. Una discrepancia de hash devuelve código 5. `--json`
emite informes estructurados y, ante errores, el contrato `{error:{code,message,hint,context}}`.
Se validó el árbol real de 3.842 archivos: todos coinciden con la referencia y `exefs/main`
empieza por `NSO0`. Alterar un byte devolvió código 5; un archivo extra solo falla con `--strict`.

El preflight rechaza destinos con contenido salvo `--force`, y comprueba el espacio antes de
crear el árbol (también durante `--dry-run`); este último no escribe ni rechaza un destino
existente. En el XCI real, `--version 1.0.0` eligió la base, mientras que `1.1.0`, `2.0.0` y
`131072` eligieron el update. Una versión no disponible devuelve `E_VERSION_NOT_FOUND`.
Los contenedores combinados base + DLC requieren `--tree`; cada título se procesa aislado y
se escribe en su propia ruta, sin exigir las claves de los otros títulos.

Cada extracción completa escribe `manifest.json` con schema 1, fuentes, título/versión,
revisión criptográfica usada y resúmenes de archivos; no contiene claves. `--resume` solo
omite archivos cuyo tamaño y SHA-256 coinciden con el journal, y rechaza cambios de fuentes o
plan (usa `--force` para iniciar de cero). Los checkpoints se guardan cada 32 archivos o 256 MiB.
Se probó interrumpiendo una extracción XCI real y reanudándola: `verify --strict` pasó y los
3.842 hashes coincidieron con la referencia.

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
| M5 — `verify`, preflight, `--version`, JSON y base+DLC combinado | ✅ validado |
| M6 — `manifest.json`, `--resume`, JSON y documentación | ✅ validado con extracción real interrumpida |
| GUI (Tauri 2 + Svelte) | ⏳ v2 (~5-6 días) |

## Documentación

- [**Diseño técnico**](DISENO-nsromexefs.md) — arquitectura, formatos, algoritmos y plan.
- [Guía de claves](docs/claves.md) — formatos admitidos, validación y manejo seguro.
- [Licencia](LICENSE) — MIT.
- [Créditos y licencias de terceros](THIRD-PARTY-NOTICES.md).

## Requisitos (versión final)

- Un volcado propio del juego (`.nsp` o `.xci`) y, si aplica, su update y DLC.
- Un archivo de claves obtenido de tu propia consola (`prod.keys` y, opcionalmente, `title.keys`).
- Nada más: el binario será autocontenido.

## Descargas

Las versiones publicadas en **Releases** incluyen paquetes portables para Windows x64, Linux
x64 (glibc) y macOS (Apple Silicon e Intel), junto con `SHA256SUMS`. Descarga el paquete de tu
plataforma, extráelo y ejecuta `nsromexefs --help`. Para verificar un paquete, compara el hash
SHA-256 del archivo con la línea correspondiente de `SHA256SUMS`.

Los tags `vX.Y.Z` crean una release en borrador tras validar la versión y compilar los paquetes.
El borrador requiere revisión y publicación manual; crear el workflow no publica una versión.

## Aviso legal

`nsromexefs` **no** distribuye claves, contenido de juegos ni herramientas de elusión.
No está afiliado a Nintendo. Uso previsto: recompilación, descompilación y preservación de juegos
que el usuario posee legalmente. La responsabilidad del uso recae en el usuario.

Las marcas mencionadas pertenecen a sus respectivos titulares.

## Licencia

MIT — ver [`LICENSE`](LICENSE).

Este proyecto reutiliza conocimiento y lógica de proyectos de código abierto; consulta
[`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md) para la atribución completa.
