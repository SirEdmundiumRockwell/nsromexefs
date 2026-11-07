# nsromexefs — Diseño técnico

> **English summary:** This is the original technical design document, kept in Spanish. It covers the container model, cryptography, extraction pipeline, CLI, GUI, testing, and release plan. Some implementation details have evolved since it was written; the [README](../README.md), the source code, and [`THIRD-PARTY-NOTICES.md`](../THIRD-PARTY-NOTICES.md) are authoritative. User-facing CLI and core messages are now emitted in English, while the GUI translates them for its Spanish locale. For user-facing documentation, start with the [README](../README.md) and the [key file guide](keys.md).

> **Estado:** v0.1 · diseño cerrado · M0-M6 implementados y validados (NSP y XCI reales)
> **Versión del documento:** 1.2
> **Fecha:** 2026-10-27 (rev. de la 1.0 del 2026-10-25)
> **Autor:** Sir Edmund Rockwell
> **Licencia:** MIT — ver [`LICENSE`](../LICENSE) y [`THIRD-PARTY-NOTICES.md`](../THIRD-PARTY-NOTICES.md)

Herramienta de línea de comandos que convierte **NSP/XCI** (base, update y DLC) en un árbol
**`exefs/` + `romfs/`** listo para cualquier proyecto de recompilación o descompilación.

**Contexto:** destilado del trabajo manual hecho para extraer *Link's Awakening* v1.0.0 y v1.1.0
(base + update BKTR). Ese caso se conserva como referencia y *golden test*, no como objetivo.

---

## Índice

1. [Resumen](#1-resumen)
2. [Alcance](#2-alcance)
3. [Casos de uso](#3-casos-de-uso)
4. [Requisitos](#4-requisitos)
5. [Modelo de formatos](#5-modelo-de-formatos)
6. [Arquitectura](#6-arquitectura)
7. [Flujo](#7-flujo)
8. [Algoritmos críticos](#8-algoritmos-críticos)
9. [Salida](#9-salida)
10. [CLI](#10-cli)
11. [Errores y diagnósticos](#11-errores-y-diagnósticos)
12. [Rendimiento y memoria](#12-rendimiento-y-memoria)
13. [Legalidad](#13-legalidad)
14. [Plan por hitos](#14-plan-por-hitos)
15. [Pruebas](#15-pruebas)
16. [Riesgos](#16-riesgos)
17. [Extensiones futuras](#17-extensiones-futuras)
18. [Claves: prod.keys y title.keys](#18-claves-prodkeys-y-titlekeys)
19. [Integridad de disco](#19-integridad-de-disco)
20. [Concurrencia y rendimiento](#20-concurrencia-y-rendimiento)
21. [Modelo de datos](#21-modelo-de-datos)
22. [Errores y verificación](#22-errores-y-verificación)
23. [Interfaz gráfica (v2)](#23-interfaz-gráfica-v2)
24. [Stack y empaquetado](#24-stack-y-empaquetado)
25. [Licencia y atribución](#25-licencia-y-atribución)
26. [ADRs y plan revisado](#26-adrs-y-plan-revisado)
- [Anexo A — Estructuras](#anexo-a--estructuras)
- [Anexo B — Caso real](#anexo-b--caso-real-links-awakening-110)
- [Anexo C — Equivalencias](#anexo-c--equivalencias-con-las-herramientas-actuales)
- [Anexo D — Glosario](#anexo-d--glosario)
- [Anexo E — Convenciones](#anexo-e--convenciones-de-nombres)

---

## 1. Resumen

`nsromexefs` es una herramienta de línea de comandos, en un solo ejecutable y sin dependencias
externas, que:

1. Lee contenedores **NSP** y **XCI** (incluidas sus particiones de update).
2. Detecta automáticamente qué es base, update y DLC de cada título.
3. Obtiene las claves de contenido desde los tickets (o desde `title.keys`), sin intervención manual.
4. Extrae **exefs** y **romfs** de la NCA de programa.
5. Si hay update, **fusiona el parche BKTR** con el romfs base y entrega el romfs final.
6. Verifica el resultado y lo deja en un árbol plano `exefs/` + `romfs/`.

El objetivo es sustituir una cadena de 15-20 comandos, dos binarios distintos y configuración
manual (`~/.switch/title.keys`, `--contentkey`, parcheo de cabeceras, NCA descifradas de 6 GB) por
**un comando**.

```console
nsromexefs dump "juego [v0].nsp" "juego [v1.1.0].nsp" -o salida -k prod.keys
```

**No se integra con nada.** No instala, no configura, no modifica otros programas: escribe un
árbol de archivos y termina. Qué se hace después con él (`exefs`/`romfs` para un recompilador,
para un emulador o para inspección) es decisión de quien lo usa.

---

## 2. Alcance

### Dentro de alcance (v1)

| Elemento | Soporte |
|---|---|
| NSP (PFS0) | Sí |
| XCI (HFS0: secure + update + normal) | Sí |
| NCA3 | Sí |
| NCA2 / NCA0 (legacy) | Lectura; extracción best-effort |
| NCA de programa (Program) | Sí — exefs + romfs (+ `logo` con `--all-parts`) |
| NCA de datos (Data, DLC) | Sí — romfs |
| NCA de control (Control), manual, meta | Extracción de su PFS0 |
| Updates BKTR | Fusión completa con base |
| DLC y sus updates | Sí — romfs aparte |
| Múltiples versiones del mismo título | Selección automática de la más alta |
| Lote (carpeta de NSPs/XCI) | Sí |
| Verificación de resultado | Sí |

### Fuera de alcance (v1)

- **NSZ / NCZ** (contenedores comprimidos). Se detectan y se avisa; no se descomprimen.
- Verificación de firmas RSA (PSS) — opcional, no bloqueante.
- Tickets personalizados (titlekey cifrado con la clave RSA de una consola): error explícito.
- Instalación en consola, MTP, generación de NSP/XCI.
- **Integración con launchers o recompiladores.** No se detectan rutas de terceros, no se copian
  archivos a instalaciones ajenas, no se comprueba si otro programa está en ejecución. Fuera.
- Interfaz gráfica: prevista para v2 (§23); la CLI cubre el caso de uso completo.

### Principio de alcance

> `nsromexefs` produce archivos. Todo lo demás es responsabilidad de quien los consume.

---

## 3. Casos de uso

| ID | Caso | Entrada | Salida |
|---|---|---|---|
| CU-1 | Juego base solo | `juego [v0].nsp` | `exefs/`, `romfs/` de la versión base |
| CU-2 | Base + update | 2 NSP | `exefs/`, `romfs/` fusionados de la versión más alta |
| CU-3 | Solo update | `[v1.1.0].nsp` | Error: falta base (código 3) |
| CU-4 | Base + update + DLC | 3+ NSP | Base/update fusionados en `exefs`/`romfs`; DLC en subcarpeta propia |
| CU-5 | XCI con update embebido | `.xci` | Igual que CU-2 leyendo la partición update |
| CU-6 | Lote | Carpeta | Un subdirectorio por título/versión (`--tree`) |
| CU-7 | Solo inspección | cualquiera | Tabla de títulos, versiones, tipos, claves disponibles |
| CU-8 | Verificación | carpeta extraída | Comprobación de layout, conteos y hashes |
| CU-9 | Uso en otro recompilador | carpeta extraída | Nada especial: el árbol es genérico |

---

## 4. Requisitos

### Funcionales

- **RF-01** Leer `prod.keys` y `title.keys` tolerando defectos: valores de 34 hex (bug de Lockpick)
  se truncan y se avisa; claves desconocidas se ignoran; comentarios, CRLF y BOM se aceptan.
- **RF-02** Derivar la clave de contenido desde `title.keys` si hay entrada para el rights id
  (descifrando el valor con `titlekek[rev]`), y si no, desde el ticket del contenedor, o desde
  el key area si no hay rights id.
- **RF-03** Detectar tipo de contenido y versión leyendo el CNMT, no el nombre del archivo.
- **RF-04** Extraer exefs (PFS0) y romfs (IVFC) de la NCA de programa.
- **RF-05** Fusionar secciones BKTR contra la NCA base, sin escribir NCA descifradas intermedias.
- **RF-06** Seleccionar automáticamente la versión más alta disponible (`--version` para forzar).
- **RF-07** Escribir `exefs/` y `romfs/` planos, más `manifest.json` y `SHA256SUMS`.
- **RF-08** Informar de progreso y de errores accionables (qué clave falta, qué archivo falla, por qué).
- **RF-09** Modo lote con reanudación.
- **RF-10** `--only-changed`: extraer solo los archivos tocados por el parche (delta).
- **RF-11** Aceptar múltiples archivos de claves en una sola invocación (`-k prod.keys -k title.keys`).
- **RF-12** Ejecutar un *preflight* completo antes de escribir el primer byte.

### No funcionales

- **RNF-01** Un solo ejecutable, sin Python/.NET/runtime externo.
- **RNF-02** Memoria O(1) por archivo (streaming por bloques); árbol romfs en memoria (miles de
  entradas, despreciable).
- **RNF-03** Rendimiento objetivo: ≥ 200 MB/s de escritura; 5,8 GB en < 60 s en NVMe.
- **RNF-04** Nunca escribir intermedios de varios GB (la NCA base no se descifra a disco).
- **RNF-05** Determinista: misma entrada → mismos bytes y mismo `manifest.json` (salvo `timestamp`).
- **RNF-06** Sin claves embebidas ni telemetría; el usuario aporta las suyas.
- **RNF-07** Ningún dato del usuario sale de la máquina.

---

## 5. Modelo de formatos

### 5.1 Contenedores

```
NSP (PFS0)
 ├── <rights_id>.tik            ticket: titlekey block
 ├── <id>.cnmt.nca              meta: contiene el CNMT (versión, tipo, lista de contenidos)
 ├── <id>.nca                   programa / datos / control / manual
 └── ...

XCI (HFS0)
 ├── cabecera gamecard 0x200    magic "HEAD" en 0x100; hfs0_offset / hfs0_header_size
 ├── root HFS0
 ├── secure/    (mismo contenido que un NSP)
 ├── update/    (NSP delta)
 └── normal/
```

PFS0: cabecera `0x10` (`PFS0`, num_files, string_table_size, reservado), luego `0x18` por entrada
(offset relativo al inicio de datos, tamaño, offset del nombre, reservado), tabla de nombres UTF-8,
datos.

HFS0: cabecera `0x10` (`HFS0`, num_files, string_table_size, reservado), luego `0x40` por entrada
(offset relativo al inicio de datos, tamaño, offset del nombre, `hashed_size`, reservado y hash
SHA-256 de 0x20), tabla de nombres UTF-8, datos.

Implementación (M4): `Hfs0::read_at` recibe el offset donde empieza el contenedor y el límite
válido, y devuelve **offsets absolutos** dentro del archivo. El XCI se lee con `Xci::read`, que
resuelve el HFS0 raíz desde `hfs0_offset`, ignora las particiones de tamaño cero (cartucho
recortado) y registra los errores de una partición sin perder las demás. Tanto `secure` como la
partición `update` pueden contener **varios CNMT** (base + update en un XCI "con update", o
base + DLC en un NSP): se abren y clasifican todos, no solo el primero. La partición `update`
se fusiona con `secure` cuando su CNMT es un `Patch` de la misma aplicación; si hay más de un
update (por ejemplo el interno del XCI y un NSP suelto) gana la versión más alta y se avisa.
Los cartuchos no suelen incluir tickets: si una NCA usa rights id y no hay ticket, la clave
debe estar en `title.keys` (`E_KEY_TITLEKEY_MISSING`). Las NCA de un cartucho genuino suelen
usar **key area**, así que `prod.keys` basta.

### 5.2 NCA

Cabecera de `0xC00` bytes, cifrada con **AES-XTS** y `header_key`:

| Offset | Campo |
|---|---|
| `0x000` | firma con clave fija (0x100) |
| `0x100` | firma NPDM (0x100) |
| `0x200` | magic `NCA3` / `NCA2` / `NCA0` |
| `0x204` | distribución (0 = digital, 1 = cartucho) |
| `0x205` | tipo de contenido (0 Program, 1 Meta, 2 Control, 3 Manual, 4 Data, 5 PublicData) |
| `0x206` | `crypto_type` |
| `0x207` | índice KAEK |
| `0x208` | tamaño total (u64) |
| `0x210` | title id (u64 LE) |
| `0x21C` | versión SDK |
| `0x220` | `crypto_type2` |
| `0x221` | generación de clave fija |
| `0x230` | rights id (0x10) |
| `0x240` | 4 × entrada de sección (0x10): `media_start`, `media_end` (× `0x200`) |
| `0x280` | 4 × SHA-256 de cabecera de sección |
| `0x300` | key area cifrada (0x40) |
| `0x400` | 4 × cabecera de sección (0x200) |

Cabecera de sección (`0x200`):

| Offset | Campo |
|---|---|
| `+0x02` | partition type (0 RomFS, 1 PFS0) |
| `+0x03` | fs type (2 PFS0, 3 RomFS) |
| `+0x04` | cifrado (1 none, 2 XTS, 3 CTR, 4 **BKTR**) |
| `+0x08` | superbloque específico: PFS0 `0x50`, RomFS/IVFC `0xE0`, BKTR `0x138` |
| `+0x140` | `section_ctr` (8 bytes, **guardado invertido**: se voltea al construir el IV) |

**Superbloque PFS0** (`+0x08` de la cabecera de sección, `pfs0_superblock_t`):
el PFS0 **no** empieza al inicio de la sección. Delante va la tabla de hashes y el sistema
de archivos arranca en `pfs0_offset`.

| Offset | Campo |
|---|---|
| `+0x00` | SHA-256 de la tabla de hashes (`master_hash`, 0x20) |
| `+0x20` | `block_size` (u32, potencia de 2) |
| `+0x24` | reservado (u32, siempre 2) |
| `+0x28` | `hash_table_offset` (u64) |
| `+0x30` | `hash_table_size` (u64) |
| `+0x38` | **`pfs0_offset`** (u64) |
| `+0x40` | `pfs0_size` (u64) |

`hash_table_offset` y `pfs0_offset` son **relativos al inicio de la sección**. En las NCA
reales de este proyecto: meta `pfs0_offset = 0x20`, exefs del programa base `0x8000`,
logo del programa base `0x4000` (y además sin cifrar). Suponer que el PFS0 empieza en
`+0x00` —o justo tras la tabla de hashes sin leer el superbloque— produce "PFS0 no
encontrado" con una criptografía perfectamente correcta.

**Comprobación barata de integridad:** `SHA-256(tabla de hashes) == master_hash`. Valida de
golpe clave de contenido, IV y offsets antes de parsear el sistema de archivos.

**Regla de oro:** nunca fiarse del tamaño declarado en la entrada de sección. El tamaño real útil
se deduce de las tablas y del `nca_size`. (Ver Anexo B: el archivo real tenía 0x4200 bytes
declarados de más.)

### 5.3 Cifrados

| Uso | Modo | Clave / IV |
|---|---|---|
| Cabecera NCA | AES-128-XTS | `header_key` (16 B datos + 16 B tweak); tweak = nº de sector BE en bytes 8..15; GF(2¹²⁸) con 0x87 |
| Secciones CTR | AES-128-CTR | `content_key`; IV = `section_ctr` (8 B) ‖ (offset absoluto >> 4) (8 B) |
| Key area | AES-128-ECB | clave derivada de `master_key[rev]` + `aes_kek_generation_source` + `aes_key_generation_source` según KAEK |
| Titlekey | AES-128-ECB | `titlekek[rev]` sobre los 16 B del bloque del ticket |
| Patch BKTR | AES-128-CTR (**CTR-EX**) | IV = `section_ctr[0..3]` ‖ `ctr_val` (u32 BE) ‖ ((inicio_sección + phys_offset) >> 4) (u64 BE) |

**Revisión de clave (crítica):**

```
rev = max(crypto_type, crypto_type2)
if rev > 0: rev -= 1        # 0 y 1 son ambos "master key 0"
titlekek[rev]  ·  master_key[rev]
```

Ejemplo real: `crypto_type = 10` → **`titlekek_09`**; `crypto_type = 8` → **`titlekek_07`**.
Equivocarse aquí produce "clave incorrecta" con un keyset perfectamente válido.

### 5.4 RomFS

```
Superbloque (0x138) = cabecera IVFC (0xE0) + relleno
Cabecera IVFC (0xE0):
  0x00 magic "IVFC"; 0x08 master_hash_size; 0x0C num_levels (declara 7; ver nota)
  0x10 level_headers[6] (0x18 c/u): logical_offset u64, hash_data_size u64, block_size u32 (log2), reservado
  0xC0 master_hash (0x20)

Cabecera RomFS (0x50): header_size, offsets/tamaños de las 4 tablas y data_offset
Entradas de directorio: parent, sibling, child, file, hash, name_size, name[]   (0x18 + nombre)
Entradas de archivo:    parent, sibling, offset, size, hash, name_size, name[]  (0x20 + nombre)
```

**`num_levels` no es de fiar.** Las NCA reales declaran **7**, pero la cabecera solo tiene
espacio para seis `level_headers`: `0x10 + 6 × 0x18 = 0xA0`, y en `0xA0` empieza el relleno
(de hecho, un séptimo nivel se solaparía con él y con `master_hash`). hactool ignora el campo
y recorre siempre `IVFC_MAX_LEVEL = 6`. Hay que hacer lo mismo: validar que el valor
declarado es plausible (`1..=7`) y leer **siempre** seis niveles. Rechazar `7` por
"demasiados niveles" aborta la extracción de cualquier NCA de Switch, con la criptografía
perfectamente correcta.

El nivel 6 de IVFC (índice 5) da `romfs_offset` y `romfs_size` **relativos al inicio de la
sección**. Los datos del romfs se leen en `section_start + romfs_offset + data_offset + file.offset`.

### 5.5 BKTR (parches de update)

La sección de un update **no contiene un romfs**, contiene:

```
[superbloque 0x138]
   cabecera IVFC 0xE0  (describe el romfs *virtual* resultante)
   relleno 0x18
   relocation_header  0x20  (offset, size, "BKTR", versión, num_entries)
   subsection_header  0x20  (offset, size, "BKTR", versión, num_entries)
[bloque de relocalización]   (en reloc.offset, tamaño reloc.size)
   cabecera 0x4000: pad, num_buckets, total_size, bucket_virtual_offsets[2046]
   buckets 0x4000 c/u: pad, num_entries, virtual_offset_end, entradas[818] de 0x14 (relleno 8 B)
       entrada: virt_offset u64, phys_offset u64, is_patch u32
[bloque de subsecciones]     (en subsec.offset)
   cabecera 0x4000: pad, num_buckets, total_size, bucket_physical_offsets[2046]
   buckets 0x4000 c/u: pad, num_entries, physical_offset_end, entradas[1023] de 0x10
       entrada: phys_offset u64, reservado u32, ctr_val u32
[datos de parche cifrados]   (solo los bloques que cambian)
```

La cabecera de cada tabla es **de tamaño fijo**: 0x10 de campos más un array de 2046
offsets de bucket (0x3FF0), es decir **0x4000 bytes**. Por eso los buckets empiezan siempre
en `0x4000` y avanzan de `0x4000` en `0x4000`, y `num_buckets ≤ 2046`. Calcular el inicio como
`0x10 + 8 × num_buckets` —lo que hace un lector ingenuo, y lo que se deduciría de la
definición de la estructura— desplaza todos los buckets y devuelve entradas basura: las
tablas reales reservan el array entero aunque solo usen las primeras posiciones.

Significado: cada entrada de relocalización cubre el rango `[virt_offset, siguiente.virt_offset)`.
Si `is_patch = 0`, ese rango **está en el romfs base**; si `is_patch = 1`, está en los datos de
parche del update, y su IV lo da la entrada de subsección cuyo rango contiene ese `phys_offset`.

Los bloques parcheados se leen del contenedor **en crudo** (ciphertext): su cifrado no es el
CTR normal de la sección, sino CTR-EX por subsección (§8.4). Descifrarlos antes con
`read_section` y volver a aplicarles CTR-EX produce basura; el síntoma típico es una cabecera
RomFS con un tamaño absurdo.

Los offsets son **relativos al inicio de la sección**, no al archivo.

### 5.6 Relación entre secciones y modelo de datos

El mapeo entre lo que se declara en el contenedor y lo que se extrae se especifica en §21
(CNMT, ticket, XCI, DLC). Este capítulo describe **bytes**; §21 describe **semántica**.

---

## 6. Arquitectura

```
nsromexefs-core (biblioteca)          nsromexefs-cli (binario)        nsromexefs-gui (v2)
 ├── container  NSP / XCI (PFS0, HFS0)  ├── clap: comandos y opciones   └── tauri + svelte
 ├── nca        cabecera, secciones     ├── códigos de salida
 ├── crypto     XTS, CTR, CTR-EX, ECB
 ├── keys       prod.keys, title.keys   └── salida humana / --json
 ├── cnmt       cabecera y contenidos
 ├── fs         PFS0 (exefs), RomFS/IVFC
 ├── bktr       tablas y mapeo virtual→físico
 ├── output     escritura atómica, manifest
 └── verify     IVFC, conteos, hashes
```

Dependencias directas (Rust y GUI):

| Crate / paquete | Uso |
|---|---|
| `aes`, `ctr`, `sha2` | criptografía |
| `serde`, `serde_json` | manifest y `--json` |
| `thiserror`, `anyhow` | errores |
| `fs2` | bloqueo de archivos |
| `clap` | CLI |
| `tauri`, `tauri-plugin-dialog` | ventana, puente y diálogos |
| `svelte`, `vite`, `typescript` | interfaz y build del frontend |

Reglas de diseño:

- **Todo lectura por streaming**: ningún componente carga una NCA ni un romfs completo en memoria.
- **Un `Read + Seek` virtual por sección**: la capa romfs no sabe si detrás hay CTR, CTR-EX o BKTR.
- **Nada de validaciones de layout que la consola no hace**: se acepta lo que el archivo declare,
  con límites de seguridad (no leer fuera del archivo).
- **El núcleo no imprime**: recibe un `ProgressSink` y devuelve resultados; CLI y GUI son adaptadores.

---

## 7. Flujo

```
F0  Descubrimiento      enumerar entradas (NSP/XCI/carpetas), abrir contenedores
F1  Clasificación        leer CNMT: title_id, versión, tipo, contenidos
F2  Agrupación           por title_id: base / update / DLC
F3  Selección            versión objetivo (más alta por defecto o --version)
F4  Claves               title.keys → tickets → content keys; verificar que están todas
F5  Preflight            espacio libre, colisiones de nombres, destino escribible
F6  Extracción           exefs (PFS0) + romfs (IVFC) de la NCA de programa
F7  Fusión               si hay update: romfs virtual = BKTR(base, update)
F8  Salida               escribir exefs/, romfs/, manifest.json, SHA256SUMS; verificar
```

Decisiones automáticas:

| Situación | Acción |
|---|---|
| Solo base | Extraer base |
| Base + update | Fusionar y entregar versión del update |
| Solo update | Error claro (código 3) |
| Varios updates | Usar el de versión más alta (o `--version`) |
| Varias bases | Usar la más alta ≤ update; avisar |
| DLC presente | Extraer en subcarpeta propia |
| Clave ausente | Error con la clave exacta que falta y la revisión necesaria |
| Destino con datos | Abortar salvo `--force` o `--resume` |

---

## 8. Algoritmos críticos

### 8.1 Derivación de claves

```
rev = max(nca.crypto_type, nca.crypto_type2)
if rev > 0: rev -= 1

si nca.rights_id != 0:
    si title_keys[rights_id] existe:
        # OJO: el valor de title.keys es el titlekey **cifrado**, igual que el
        # bloque del ticket. hactool lo imprime como "Titlekey (Encrypted)" y
        # nstool lo guarda en external_enc_content_keys. No es la clave lista.
        content_key = AES-ECB-decrypt(titlekek[rev], title_keys[rights_id])
    sino:
        tik = ticket_con_rights_id(rights_id)            # del NSP/XCI
        content_key = AES-ECB-decrypt(titlekek[rev], tik.titlekey_block)
sino:
    # key area descifrada = 0x40 B = dos ranuras de 0x20:
    #   ranura 0 (0x00..0x20): clave XTS de 0x20 bytes
    #   ranura 1 (0x20..0x40): sus primeros 16 B son la clave AES-CTR
    key_area    = AES-ECB-decrypt(kaek[rev][kaek_ind], nca.encrypted_keys)
    content_key = key_area[0x20..0x30]
```

Si `title.keys` y el ticket discrepan, **gana `title.keys`** y se emite un aviso con ambos
rights id. Con un `title.keys` correcto ambas rutas producen exactamente la misma clave; si
no coinciden, casi siempre el archivo está corrupto o la revisión de `titlekek` es otra.

### 8.2 Cabecera NCA (XTS)

```
k1 = header_key[0..16]; k2 = header_key[16..32]
para cada sector s de 0x200:
    T = AES-ECB-encrypt(k2, BE64(s) en bytes 8..15)
    para cada bloque b de 16:
        si b > 0: T = mul_alpha(T)                  # GF(2^128), 0x87
        P = AES-ECB-decrypt(k1, C xor T) xor T
```

### 8.3 CTR de secciones

```
IV = section_ctr(8 B) || BE64(offset_en_la_NCA >> 4)
descifrar bloque a bloque (0x10), avanzando el contador como entero BE de 128 bits
```

Dos matices que cuestan horas de depuración:

- **`section_ctr` está guardado al revés.** En disco son dos u32 LE (`generation` en
  `+0x140`, `secure_value` en `+0x144`); el IV se construye escribiéndolos como u64 LE y
  **volteando los 8 bytes**. No es "leer 8 bytes y usarlos tal cual".
- **El offset es relativo al inicio de la NCA, no al contenedor.** Si la NCA vive en un NSP
  en el offset `F`, el `seek` físico es `F + offset`, pero el contador usa `offset >> 4`
  (más el `section_ctr`). Mezclar ambas bases descifra basura solo en las NCA que no
  empiezan en el byte 0 del contenedor — que son casi todas.

### 8.4 Fusión BKTR (núcleo)

```
leer superbloque (0x138) al inicio de la sección
reloc  = leer_bloque(reloc.offset, reloc.size)      # CTR normal de la sección
subsec = leer_bloque(subsec.offset, subsec.size)    # CTR normal de la sección

# Normalización (importante):
#  - las entradas pueden venir en orden descendente; ordenar por virt_offset ascendente
#  - añadir una entrada centinela por bucket con el virt_offset del bucket siguiente
#  - en subsecciones, añadir centinela con el offset final

para cada tramo virtual v (hasta ivfc.size):
    r = buscar_reloc(v)                  # binaria sobre virt_offset
    n = min(resto, r.siguiente.virt_offset - v)   # el tramo lo cortan las relocalizaciones
    phys = r.phys_offset + (v - r.virt_offset)
    si r.is_patch:
        s    = buscar_subsec(phys)           # binaria sobre phys_offset
        alin = phys & !0xF                   # el IV usa el offset físico alineado
        iv   = section_ctr[0..4] || BE32(s.ctr_val) || BE64((section_start + alin) >> 4)
        datos = leer_crudo(section_start + phys, n)      # ciphertext, sin descifrar
        datos = AES-CTR(content_key, iv, saltar = phys - alin, datos)
    sino:
        # leer del base, en la posición r.phys_offset (== v en la práctica)
        datos = leer_base(phys, n)           # ya descifrado
    emitir(datos)
```

Detalles que hay que respetar:

- **Los bloques parcheados se leen en crudo.** Su cifrado es CTR-EX por subsección, no el CTR
  normal de la sección; descifrarlos primero con `read_section` y volver a aplicar CTR-EX
  produce basura. Hace falta una lectura que devuelva el ciphertext tal cual está en el
  contenedor (en el código, `SectionRead::read_raw_at`).
- El contador usa el **offset físico alineado a 0x10** (`(section_start + phys) & !0xF`), no el
  virtual, y luego se salta `phys & 0xF` bytes del keystream. Usar el offset sin alinear
  descifra bien solo cuando el tramo empieza en múltiplo de 16 (el caso habitual, y por eso el
  error pasa desapercibido hasta que un parche cae a mitad de bloque).
- El tamaño del tramo lo fija la **siguiente relocalización**, no un bloque fijo de 0x4000:
  una relocalización puede empezar y terminar dentro de un bloque de 0x4000.
- **No validar** que `subsec.offset + subsec.size == tamaño de sección`, ni que
  `subsec.total_size == subsec.offset`. Los archivos reales no siempre lo cumplen (Anexo B) y la
  consola no lo exige.
- `reloc.total_size` (y `ivfc.size`) dan el tamaño del romfs virtual completo (aquí
  6.232.003.903 bytes), mayor que el tamaño físico de la sección.
- Si no hay base, solo se pueden extraer los bloques de parche (`--delta`, futuro).

### 8.5 RomFS

```
romfs_base = section_start + ivfc.level[5].offset
hdr = leer(romfs_base, 0x50)
meta_dir  = romfs_base + hdr.dir_meta_table_offset
meta_file = romfs_base + hdr.file_meta_table_offset
datos     = romfs_base + hdr.data_offset

recorrer(entry_dir=0, path=""):
    para cada hijo (child → sibling):
        si es directorio: crear y recursar
        si es archivo:   copiar (datos + file.offset, file.size) a path
```

Los nombres son UTF-8; `name_size = 0` significa entrada sin nombre (se omite). El saneado de
nombres y las colisiones se definen en §19.

### 8.6 Detección de exefs

Una sección PFS0 de una NCA Program es exefs si contiene `main.npdm`. Si además contiene `main`
con magic `NSO0`, es un ejecutable válido para recompilar.

---

## 9. Salida

### 9.1 Por defecto: plana

```console
nsromexefs dump base.nsp update.nsp -o salida
```

```
salida/
  exefs/            main, main.npdm, rtld, sdk, subsdk0, ...
  romfs/            árbol completo fusionado
  manifest.json
  SHA256SUMS
```

Es lo que espera cualquier proyecto de recompilación: dos carpetas con esos nombres exactos.

En v0.1, cada extracción completa escribe `manifest.json` y `SHA256SUMS` (§9.3). El manifest
se escribe al final; su presencia indica que el árbol terminó correctamente.

### 9.2 `--tree`: un directorio por título y versión

```
salida/
  <app_id>/
    v1.1.0/
      exefs/  romfs/  SHA256SUMS
    dlc/<dlc_id>/v0/
      romfs/  SHA256SUMS
```

- `<app_id>` es el title id de la **aplicación** (para un update, el de su base; para un DLC,
  el del juego al que pertenece), de modo que base, update y DLC quedan juntos.
- `<versión>` es la versión visible de `control.nacp` cuando se puede leer; si no, el entero
  del CNMT. Se sanea (solo `A-Z a-z 0-9 . - _`) antes de usarla como componente de ruta.
- Obligatorio en modo lote (si no, dos juegos se pisarían). `batch` lo fuerza siempre.

### 9.3 `manifest.json`

```json
{
  "schema": 1,
  "title_id": "01006BB00C6F0000",
  "version": "1.1.0",
  "version_raw": 131072,
  "sources": ["...v0.nsp", "...v1.1.0.nsp"],
  "merged": true,
  "crypto_revision": 9,
  "keys_source": ["prod.keys"],
  "exefs": { "files": 6, "main_sha256": "...", "main_size": 14983439 },
  "romfs": { "files": 3836, "bytes": 6232003903, "changed_files": 1 },
  "sanitized_paths": [],
  "skipped_entries": [],
  "plan_hash": "sha256 del plan de extracción",
  "timestamp": "2026-10-27T12:00:00Z"
}
```

- `schema` permite evolucionar el formato sin romper consumidores.
- `version` es la **versión visible** y sale de `control.nacp` (`0x3060`, 16 bytes), no del
  CNMT. `version_raw` es el entero interno del CNMT (aquí `131072`), útil para comparar y
  ordenar versiones pero no para mostrar.
- `crypto_revision` es la **revisión de master key** de la NCA de programa seleccionada, no el
  `crypto_type` crudo ni la revisión máxima incluida en `prod.keys`.
- Nunca contiene claves, ni rutas de claves, ni contenido de archivos.

`SHA256SUMS` en formato estándar (`<hash>  <ruta>`), verificable con `sha256sum -c` o
`Get-FileHash` + comparación.

---

## 10. CLI

```
nsromexefs info   <archivos...>                    # títulos, versiones, tipos, claves disponibles
nsromexefs dump   <archivos...> [-o DIR]           # extrae (fusiona si hace falta)
nsromexefs verify <DIR>                            # valida un árbol exefs/romfs
nsromexefs batch  <DIR> -o <OUT>                   # lote
nsromexefs keys   --check <archivo.keys>           # valida y sanea un keyset
```

### Opciones de `dump`

| Opción | Efecto |
|---|---|
| `-k, --keys <file>` | archivo de claves; **repetible** (`prod.keys`, `title.keys`) |
| `-o, --out <dir>` | destino (por defecto `./<title_id>`) |
| `--version <v>` | fuerza versión (por defecto la más alta) |
| `--tree` | layout por título/versión (obligatorio en lote) |
| `--only-changed` | solo archivos tocados por el parche |
| `--all-parts` | incluye particiones extra (p. ej. `logo`) |
| `--layout exefs+romfs\|romfs\|nca` | qué generar (`nca` = NCA descifrada, para depurar) |
| `--jobs <n>` | paralelismo (por defecto núcleos físicos) |
| `--resume` | reanuda una extracción interrumpida |
| `--force` | sobrescribe un destino con datos |
| `--dry-run` | muestra el plan sin escribir |
| `--json` | salida legible por máquina |
| `--strict` | falla ante cualquier discrepancia de hash |
| `--quiet` / `-v` | silencio / detalle |

En v0.1 (M0-M6) están implementadas `-k`, `-o`, `--tree`, `--version`, `--exefs-only`,
`--romfs-only`, `--force`, `--dry-run`, `--json`, `--resume` y `-v`. `--only-changed`,
`--all-parts`, `--layout` y la paralelización `--jobs` siguen pendientes para una versión
posterior.

`dump` acepta **uno o dos** contenedores: la base y, opcionalmente, su update. El orden es
indiferente y la clasificación se hace por el CNMT. Un update sin base termina con
`E_BASE_MISSING` (código 3). Un `.xci` aporta en una sola invocación su base (`secure`) y su
update interno cuando corresponde.

### Opciones de `batch`

```
nsromexefs batch <DIR> [-o DIR] [--exefs-only|--romfs-only] [--dry-run] [--force] [-v] [--json]
```

| Opción | Efecto |
|---|---|
| `DIR` | carpeta que se recorre **recursivamente** en busca de `.nsp`/`.xci` |
| `-o, --out <dir>` | raíz de salida (por defecto `./nsromexefs-out`) |
| `--exefs-only` / `--romfs-only` | limitan lo extraído |
| `--dry-run` | plan completo sin escribir |
| `--force` | permite reemplazar extracciones previas |
| `-v` | lista cada archivo |
| `--json` | resumen legible por máquina |

El layout de árbol es siempre obligatorio. Los `.nsz`/`.xcz` se reportan como omitidos y los
enlaces simbólicos se ignoran. Un fallo por título no aborta el lote: al final se devuelve
`E_BATCH_FAILED` con el recuento y un código agregado (el código común si todos los fallos
coinciden; `1` si son mixtos).

### Opciones de `verify`

```
nsromexefs verify <DIR> [--strict] [--json]
```

| Opción | Efecto |
|---|---|
| `DIR` | árbol con `exefs/` y `romfs/`; si no tiene `SHA256SUMS`, busca raíces recursivamente |
| `--strict` | los archivos sobrantes y un `main` sin `NSO0` también hacen fallar |
| `--json` | informe legible por máquina, con listas completas |

Relee cada archivo listado en `SHA256SUMS`, comprueba su SHA-256, detecta rutas inválidas
(`..`, absolutas, `\`, `:`), faltantes y sobrantes. No sigue enlaces simbólicos. Termina con
código `5` si algún árbol falla. En modo recursivo, cada `SHA256SUMS` encontrado es una raíz
independiente y no se desciende dentro de ella.

### Opciones globales

`--keys` (repetible), `--help`, `--version`.

### Códigos de salida

`0` OK · `1` error de E/S · `2` faltan claves · `3` falta base para un update ·
`4` formato no soportado · `5` verificación fallida · `6` colisión de nombres · `7` espacio
insuficiente.

---

## 11. Errores y diagnósticos

| Síntoma | Mensaje |
|---|---|
| `prod.keys` con 34 hex | `aviso: master_kek_source_06 tiene 34 hex; se usa 32 (bug de Lockpick)` |
| Rights ID sin ticket ni title.keys | `falta el titlekey de 01006bb0…0008: aporta el ticket o una entrada en title.keys` |
| Falta `titlekek` de la revisión | `falta titlekek_09 (revisión de master key 9; la NCA declara crypto_type 10)` |
| Update sin base | `falta la base: para un update hay que indicar también su contenedor base` |
| BKTR sin tablas | `sección BKTR sin bloque de subsecciones; no se puede fusionar` |
| NSZ/NCZ | `contenedor comprimido no soportado (v1)` |
| Ticket personalizado | `ticket personalizado: el titlekey está cifrado con la clave RSA de una consola` |
| Colisión de nombres | `conflicto: "Data/Config" y "data/config" colisionan en Windows (ver --rename-on-collision)` |
| Espacio insuficiente | `faltan 3,1 GB libres en C: (se necesitan 6,3 GB)` |

El catálogo con códigos estables está en §22.

---

## 12. Rendimiento y memoria

Medido en el caso real (Ryzen 5 9600X, NVMe):

| Operación | Tiempo | Observación |
|---|---|---|
| hactool, base → romfs (con NCA descifrada intermedia) | 15 s + 12 s de escritura de 6,2 GB | 2 pasadas, 12 GB de disco |
| hactool, update fusionado | 7 s (tras parchear cabecera) | requiere base descifrada |
| nstool, update fusionado | 44 s | sin intermedios |
| **nsromexefs, base + update fusionado** (6,26 GB) | **12,3 s** | una pasada, sin intermedios, ~510 MB/s |

Objetivo de `nsromexefs`: una sola pasada, sin intermedios, 5,8 GB en ~30-50 s, memoria < 100 MB.
Medido en el caso real: 6,26 GB en 12,3 s, ~510 MB/s, muy por encima del objetivo.
El modelo de concurrencia previsto para M6 está en §20.

---

## 13. Legalidad

- No se incluyen claves; el usuario aporta su `prod.keys`/`title.keys` y sus propios volcados.
- No se distribuye contenido de juegos.
- El código es una reimplementación de formatos documentados públicamente.
- Uso previsto: recompilaciones, descompilaciones y preservación de juegos que el usuario posee.
- La herramienta no elude medidas tecnológicas por sí misma: no trae claves ni las obtiene de una
  consola.

---

## 14. Plan por hitos

| Hito | Contenido | Criterio de aceptación | Estimación | Estado |
|---|---|---|---|---|
| **M0** | CLI, parser de claves (`prod.keys`/`title.keys`), PFS0/NSP, `info` | lista los NSP con título, versión y tipo | 1 d | ✅ |
| **M1** | Cabecera NCA (XTS), key area, tickets, `title.keys` | muestra secciones y content key correcta | 1 d | ✅ |
| **M2** | exefs + romfs base (CTR) + escritura atómica | 3.836 archivos y `main` de 14.984.660 B idénticos a la referencia | 1,5 d | ✅ |
| **M3** | BKTR: tablas + CTR-EX + fusión base+update | 3.842 archivos con SHA256 idéntico al árbol de referencia | 1,5 d | ✅ |
| **M4** | XCI/HFS0, lote (`batch`), `--tree` | `dump` de un `.xci` y de una carpeta completa | 1 d | ✅ |
| **M5** | Preflight, verificación, `--version`, errores JSON, base+DLC combinado | mensajes de §11/§22 y `verify` OK | 1 d | ✅ |
| **M6** | `manifest.json`, `--resume`, JSON y documentación | recupera una extracción interrumpida y verifica el resultado | 1 d | ✅ |

**M2 validado** (NSP real de *Link's Awakening* v1.0.0, 6,27 GB): 6 archivos en `exefs`
(`main` = 14.984.660 B) y 3.836 en `romfs` (6.232.003.695 B). Frente al árbol 1.1.0 de
referencia extraído con hactool/nstool, **3.835 archivos son idénticos byte a byte**; el
único distinto es `region_common/event/SinkingSword.bfevfl` (2.712 B base vs 2.920 B update),
el archivo que toca el parche. El total difiere exactamente en esos 208 B.

**M3 validado** (mismos base + update 1.1.0 reales, en un solo comando): 6 archivos en
`exefs` (`main` = 14.983.439 B) y 3.836 en `romfs` (6.232.003.903 B, 169 directorios).
Comparado con el árbol 1.1.0 de referencia, **3.842/3.842 archivos idénticos por SHA-256,
0 distintos**; extracción completa en ~12 s (`cargo build --release`). Las tablas reales del
update encajan con lo descrito en §5.5 (7 relocalizaciones, 4 parcheadas, `total_size`
0x17433AC00) aplicando solo las excepciones de layout ya previstas en ADR-03.

**M4 validado**: `batch` sobre la carpeta con los NSP reales de base + update produjo
3.842/3.842 archivos idénticos por SHA-256 al árbol de referencia en 12,9 s, con layout
`01006BB00C6F0000/v1.1.0`.

Después se validó con un **XCI real** (*Link's Awakening* 1.1.0, 6,31 GB) que resultó ser un
cartucho "con update": `secure` contiene 11 entradas y **dos CNMT** (base v1.0.0 + update
v1.1.0), mientras que la partición `update` está vacía (512 B). `info` mostró las 4
particiones y ambos títulos; `dump --tree` y `batch` fusionaron el update interno y dieron
**3.842/3.842 archivos idénticos en ~12 s usando solo `prod.keys`** (base por key area,
update con ticket). La prueba descubrió que la CLI solo leía el primer CNMT de la tabla:
se corrigió para abrir **todos** los CNMT de `secure` y de la partición `update`. El XCI
sintético anterior cubre además el caso de un contenedor sin tickets, que exige `title.keys`.
El caso de lote con solo un update devuelve `E_BASE_MISSING` por título y termina en
`E_BATCH_FAILED` (código 3).

**M5 validado** (mismo XCI real y árbol de referencia):

- `verify` sobre los 3.842 archivos terminó en ~4 s con **3.842/3.842 OK**. Alterar un byte
  de un archivo produjo `E_VERIFY_FAILED` y **código 5**; un archivo sobrante solo falló con
  `--strict`.
- El preflight rechazó una segunda extracción sin `--force` (`E_OUTPUT_EXISTS`), no falló en
  `--dry-run` y aceptó `--force`. El espacio se comprueba con datos + 1 % + 64 MiB y está
  cubierto por pruebas unitarias (no había un volumen pequeño para forzar `E_NO_SPACE`).
- `--version 1.0.0` extrajo la base sola; `1.1.0`, `2.0.0` y `131072` extrajeron el update;
  una versión inexistente devolvió `E_VERSION_NOT_FOUND` con la lista disponible.
- `dump --tree` y `batch` volvieron a producir **3.842/3.842 archivos idénticos** a la
  referencia en ~12 s, y `verify` recursivo del lote terminó OK.
- Un archivo combinado base + DLC se extrae con `--tree`, abriendo solo el título pedido y
  sus parches. No había un contenedor combinado real: el filtro por título y la selección de
  versión están cubiertos con pruebas unitarias.
- Los errores con `--json` siguen el contrato `{error:{code,message,hint,context}}` y
  `dump --json` devuelve `{count,titles:[...]}`.

**M6 validado** (XCI real de *Link's Awakening*):

- La extracción completa generó `manifest.json` (schema 1), con versión visible/CNMT,
  fuentes, revisión real de la NCA de programa, tamaños, hashes y timestamp UTC; no guarda
  secretos ni rutas de claves.
- `verify --strict` validó manifest, `SHA256SUMS` y `NSO0`; archivos y manifest pasaron.
- Se terminó a la fuerza una extracción tras escribir ~1,2 GB. El journal tenía checkpoints
  por archivo y quedó un `.part`; `dump --resume` lo limpió, reanudó y eliminó el journal al
  completar. Los 3.842 hashes generados coincidieron con los del árbol de referencia.
- Reanudar un árbol ya completo a partir de `SHA256SUMS` validó los archivos existentes y lo
  completó sin reescribir los datos (7,9 s incluyendo la verificación SHA-256).
- `cargo test --all` pasó (109 pruebas), `clippy` limpio y build release correcto.

Total: **~8 días** de implementación del CLI. M3 era el único con riesgo real; el resto es mecánico.

GUI (v2): **~5-6 días** adicionales (§23).

---

## 15. Pruebas

1. **Vectores unitarios**
   - XTS: descifrar la cabecera real y comprobar `NCA3` en `0x200`.
   - CTR: descifrar la tabla BKTR y comprobar `magic == "BKTR"`, `num_buckets == 1`.
   - Fusión BKTR: sección sintética con un tramo de base y otro parcheado; el tramo parcheado
     se descifra con CTR-EX, incluido un acceso no alineado a 0x10.
   - GF(2¹²⁸): multiplicación por α contra valores conocidos.
   - Revisión: `crypto_type = 10` → `titlekek_09`.
2. **Golden tests** (los más valiosos)
   - Base: comparar árbol y hashes contra la extracción de hactool.
   - Update: comparar contra nstool 1.9.2 (ya validado: 3.842/3.842 SHA256 iguales).
3. **Matriz de formatos**: 3-5 juegos con update, 1 DLC, 1 XCI, 1 juego sin update, 1 con sección
   de layout no canónico (el caso de este proyecto).
4. **Robustez**: entradas truncadas/falsas no deben provocar pánico ni escrituras fuera de ruta
   (`..`, rutas absolutas, nombres reservados de Windows). Fuzzing en M6.
5. **Determinismo**: dos ejecuciones producen el mismo `manifest.json` salvo `timestamp`.

---

## 16. Riesgos

| Riesgo | Impacto | Mitigación |
|---|---|---|
| Variantes de layout BKTR no canónicas | fusión incorrecta o error | no validar de más; tests con casos reales; `--debug-blocks` |
| NCZ/NSZ | no soportado | detectar y avisar (v2) |
| NCA0/NCA2 | extracción parcial | implementar lectura; marcar best-effort |
| Tickets personalizados | imposible sin consola | error claro |
| Cambios de cifrado futuros | herramienta obsoleta | tablas de claves externas, sin constantes embebidas |
| Rutas hostiles en romfs | escritura fuera del destino | saneado + verificación de prefijo (§19) |
| Distribución del binario | dudas legales | sin claves, sin contenido, aviso de uso |

---

## 17. Extensiones futuras

- Interfaz gráfica con arrastrar y soltar (§23).
- `--verify-hashes` con las tablas IVFC completas.
- Soporte NCZ/NSZ.
- Exportación de NCA descifradas y de exheader para depuración.
- Perfiles de salida configurables (por ejemplo, renombrar `romfs` a otra carpeta).
- Modo servidor local (una carpeta compartida en red).

---

## 18. Claves: prod.keys y title.keys

La herramienta **no trae claves**. Todo el cifrado depende de que el usuario aporte las suyas.

### 18.1 Dos archivos, dos propósitos

| Archivo | Contenido | Formato |
|---|---|---|
| `prod.keys` | claves de consola: `header_key`, `master_key_NN`, `titlekek_NN`, `aes_kek_generation_source`, `key_area_key_application_NN`, … | `nombre = hex` |
| `title.keys` | el titlekey **cifrado** de cada rights id (igual que el bloque del ticket) | `rights_id = titlekey_cifrado` (32 + 32 hex) |

`title.keys` es **opcional**: si falta, el titlekey se obtiene del ticket del propio
contenedor. Su valor **no** es la clave de contenido: hay que descifrarlo con
`titlekek[rev]`, exactamente como el bloque del ticket (hactool lo muestra como
"Titlekey (Encrypted)"). Sirve para casos donde no hay ticket (algunos XCI) o para forzar
una clave concreta.

### 18.2 Selección y descubrimiento

`--keys` es **repetible** y se pueden mezclar ambos tipos:

```console
nsromexefs dump juego.nsp -o salida -k prod.keys -k title.keys
```

Orden de búsqueda si no se pasa `--keys` (el primero que exista se usa; se pueden acumular varios):

1. `--keys` en la línea de comandos (todos, en orden).
2. Variable de entorno `NSROMEXEFS_KEYS` (rutas separadas por `;` en Windows, `:` en Unix).
3. `prod.keys` y `title.keys` junto al ejecutable.
4. `~/.switch/prod.keys` y `~/.switch/title.keys`.

Se detecta el tipo de archivo por su contenido, no por el nombre: una línea
`rights_id = titlekey` (dos valores de 32 hex) es `title.keys`; `nombre = hex` con nombre conocido
es `prod.keys`.

### 18.3 Tolerancia de formato

- `nombre = valor`, espacios libres alrededor del `=` y del valor.
- Comentarios con `#`, líneas vacías, CRLF, BOM UTF-8.
- Valores de **34 hex** (bug de Lockpick): se truncan a 32 y se emite aviso con el nombre exacto.
- Valores de longitud incorrecta distintos de 34: se descartan con aviso.
- Claves desconocidas: se ignoran silenciosamente (los keysets traen más de lo que usamos).
- Nombres alternativos aceptados: `titlekek_09` / `title_key_09`, `key_area_key_application_09` /
  `kaek_app_09`.

### 18.4 Validación por revisión

`nsromexefs keys --check prod.keys` responde a la única pregunta que importa:

```
master_key      OK    hasta revisión 20
titlekek        OK    hasta revisión 20
header_key      OK
key_area_key    OK    aplicación, hasta revisión 20
titlekeys       0 entradas (opcional)
```

Y en el flujo normal, el preflight comprueba **la revisión exacta que necesita cada NCA**:

```
falta titlekek_09 — la NCA del update declara crypto_type 10
```

Nunca un "clave incorrecta" genérico.

### 18.5 Seguridad

- Las claves **nunca** se escriben en logs, manifest, journal ni en la configuración.
- El manifest solo anota `keys_source` (nombre del archivo) y `crypto_revision`.
- No hay red: la herramienta no hace ni una petición.
- Al mostrar errores se imprimen como mucho los 8 primeros hex de un rights id cuando es necesario
  para identificarlo.

### 18.6 Documentación de usuario

La guía práctica ([`keys.md`](keys.md)) explica:

- De dónde salen legalmente las claves (volcado de la propia consola; nunca se piden a terceros).
- Qué claves se necesitan para qué (cabecera, secciones, tickets, key area).
- El bug de los 34 hex y cómo reconocerlo.
- Qué hacer si falta una revisión (actualizar el keyset, no "probar suerte").

---

## 19. Integridad de disco

### 19.1 Nombres y rutas

El romfs es **case-sensitive**; NTFS no. Sin control, dos entradas pueden pisarse en silencio.

Reglas de saneado (se aplican a cada componente de la ruta):

| Caso | Acción |
|---|---|
| `\ / : * ? " < > \|` | se sustituyen por `_` |
| Nombres reservados (`CON`, `PRN`, `AUX`, `NUL`, `COM1..9`, `LPT1..9`) | se prefija con `_` |
| Punto o espacio final | se elimina (y se registra) |
| Componente vacío o `.` / `..` | se descarta; se registra en `skipped_entries` |
| Ruta resultante > 240 caracteres | se usa prefijo `\\?\`; si aun así excede el máximo, error |
| Dos rutas iguales ignorando mayúsculas | **error** (código 6) con la lista; `--rename-on-collision` añade `~1`, `~2` |

Todo saneado se registra en `manifest.json → sanitized_paths`, para que el usuario sepa qué se
renombró y por qué.

### 19.2 Escritura atómica

1. Cada archivo se escribe en `<nombre>.nrx-tmp` dentro del destino final.
2. Se calcula su SHA-256 mientras se escribe.
3. `rename()` al nombre definitivo (atómico en el mismo volumen).
4. Al terminar todo el árbol, se escribe `manifest.json`; **su existencia es la marca de "árbol
   completo"**.

Si el proceso muere a mitad, no hay `manifest.json` y los `.nrx-tmp` se limpian en la siguiente
ejecución.

### 19.3 Journal y reanudación

`.nsromexefs-journal` en la raíz del destino:

```json
{
  "schema": 1,
  "source_fingerprint": "sha256 de (tamaño + mtime + nombre) de cada entrada",
  "plan_hash": "sha256 del plan",
  "completed": { "romfs/region_common/event/x.bfevfl": "hash" },
  "bytes_written": 3821000000
}
```

- `--resume` valida que las fuentes y el plan actual coinciden (`source_fingerprint` y
  `plan_hash`); si no, pide `--force` para empezar de nuevo.
- Cada archivo se compromete atómicamente y su tamaño + SHA-256 se registra en el journal. Para
  no reescribir/fsync el journal miles de veces durante una extracción grande, se persiste al
  primer archivo y después cada 32 archivos o 256 MiB; tras una caída se pueden repetir como
  máximo 31 archivos desde el último checkpoint.
- Antes de omitir un archivo, se comprueban tipo de archivo, tamaño y SHA-256; si no coincide,
  se vuelve a extraer.
- Al terminar se borra el journal.

### 19.4 Preflight (antes de escribir)

| Comprobación | Si falla |
|---|---|
| Espacio libre ≥ bytes del plan + 5% | código 7, con números concretos |
| Destino existe y es escribible | código 1 |
| Destino vacío, o `--force`/`--resume` | error con instrucción |
| Claves de todas las revisiones necesarias | código 2, nombrando la clave |
| Base presente si hay update | código 3 |
| Colisiones de nombres | código 6, con la lista |
| Sistema de archivos soporta el tamaño máximo de archivo | error |

`--dry-run` ejecuta todo esto y emite el plan en JSON sin escribir nada.

### 19.5 Modo lote

- `batch` recorre la carpeta de entrada de forma recursiva (sin seguir enlaces simbólicos) y
  clasifica cada `.nsp`/`.xci` leyendo **solo el CNMT**; no necesita la titlekey del programa
  para agrupar.
- Agrupa por aplicación: elige la base y el update de versión más alta y todos los DLC (el más
  alto de cada `dlc_id`). Un update del XCI y otro NSP del mismo juego no se duplican.
- Un subdirectorio por título/versión (`--tree` obligatorio; `batch` lo fuerza).
- Un fallo en un título **no aborta** el lote: se registra y se continúa.
- Los `.nsz`/`.xcz` se reportan como omitidos (no se descomprimen en v1).
- Resumen final con `ok`/`error`/`omitido` por título y código de salida agregado: si todos los
  fallos comparten código se devuelve ese; si son mixtos, `1`.

---

## 20. Concurrencia y rendimiento

### 20.1 Por qué se puede paralelizar

CTR y CTR-EX son **random-access**: el keystream de un offset se calcula a partir de su número de
bloque, sin depender de los anteriores. Eso permite descifrar **rangos disjuntos en paralelo**.

```
hilo 0: [0x0000000, 0x400000)    hilo 1: [0x400000, 0x800000)   ...
```

### 20.2 Modelo

| Elemento | Decisión |
|---|---|
| Unidad de trabajo | bloque de **4 MB** del romfs virtual |
| Pool | `--jobs`, por defecto núcleos físicos (no lógicos) |
| Buffer | 1 por hilo, reutilizado; techo de 100 MB en total |
| Escritura | cada hilo escribe su propio archivo o su propio rango ya resuelto |
| Secuencial | lectura de tablas BKTR, recorrido del árbol y escritura del manifest |
| Orden de archivos | el plan se ordena por offset de datos para maximizar lectura secuencial |
| Windows | `FILE_FLAG_SEQUENTIAL_SCAN`; `POSIX_FADV_SEQUENTIAL` en Unix |

### 20.3 Casos especiales

- **Archivos que comparten datos** (mismo offset en el romfs): se extraen por separado; el
  paralelismo puede duplicar lecturas del mismo bloque. Se detecta y se marca en el plan para
  agruparlas en el mismo hilo.
- **Archivos muy pequeños**: se agrupan por bloque de 4 MB para no crear una tarea por archivo.
- **`--only-changed`**: el plan solo incluye los rangos con `is_patch = 1`; el resto no se lee.

### 20.4 Presupuesto y medición

| Fase | Objetivo | Cómo se mide |
|---|---|---|
| Descubrimiento + CNMT | < 1 s | `--json` → `timings.discovery_ms` |
| Claves | < 100 ms | `timings.keys_ms` |
| exefs | < 200 ms | `timings.exefs_ms` |
| romfs | ≥ 200 MB/s | `timings.romfs_ms`, `bytes/s` |
| Verificación | < 3 s | `timings.verify_ms` |

Los tiempos se emiten siempre con `--json` y con `-v`: son el test de regresión de rendimiento.

---

## 21. Modelo de datos

### 21.1 CNMT (PackagedContentMeta)

El contenido del `.cnmt` **no tiene magic**: empieza directamente con esta cabecera de `0x20`.

```c
struct sContentMetaHeader {                     // 0x20 bytes
    u64 id;                                     // 0x00  title id
    u32 version;                                // 0x08  versión (major<<16 | minor<<8 | micro)
    u8  type;                                   // 0x0C  ContentMetaType
    u8  reserved_0;                             // 0x0D  (>=17.0.0: platform)
    u16 exhdr_size;                             // 0x0E
    u16 content_count;                          // 0x10
    u16 content_meta_count;                     // 0x12
    u8  attributes;                             // 0x14
    u8  storage_id;                             // 0x15
    u8  install_type;                           // 0x16
    u8  install_state;                          // 0x17
    u32 required_download_system_version;       // 0x18
    u8  reserved_1[4];                          // 0x1C
};
```

Después: `exhdr_size` bytes de cabecera extendida, `content_count` entradas de contenido y
`content_meta_count` entradas de metadatos.

```c
struct sContentInfo {                           // 0x38 bytes
    u8 hash[0x20];          // 0x00 SHA-256 del contenido
    u8 content_id[0x10];    // 0x20
    u32 size_lower;         // 0x30
    u16 size_higher;        // 0x34  tamaño = size_higher<<32 | size_lower
    u8  content_type;       // 0x36
    u8  id_offset;          // 0x37
};

struct sContentMetaInfo {                       // 0x10 bytes
    u64 id;                 // 0x00
    u32 version;            // 0x08
    u8  type;               // 0x0C
    u8  attributes;         // 0x0D
    u8  reserved[2];        // 0x0E
};
```

Enumeraciones relevantes:

| `ContentMetaType` | Valor | `ContentType` (CNMT) | Valor |
|---|---|---|---|
| Application | `0x80` | Meta | `0` |
| Patch | `0x81` | Program | `1` |
| AddOnContent | `0x82` | Data | `2` |
| Delta | `0x83` | Control | `3` |
| DataPatch | `0x84` | HtmlDocument | `4` |
| | | LegalInformation | `5` |
| | | DeltaFragment | `6` |

Este `ContentType` es `nn::ncm::ContentType` y **no** tiene nada que ver con el byte
`ContentType` de la cabecera NCA (`0` Program, `1` Meta, `2` Control…). Confundirlos hace
que el manual legal aparezca como "DataPatch".

Los `DeltaFragment` suelen **no viajar en el NSP**: son los bloques que la consola pide
aparte para el parche delta. Se listan como "fuera del contenedor", no como error.

**Versión:** el `version` del CNMT es el entero interno de Nintendo, no el número que ve el
usuario. nstool lo decodifica como
`(v>>26)&0x3F . (v>>20)&0x3F . (v>>16)&0xF - (v>>8)&0xFF . v&0xFF`; con `131072`
(`0x20000`) sale `0.0.2-0.0`, no `1.1.0`. La versión visible (`1.1.0`, `1.0.0`) está en
`control.nacp`, en `0x3060` (16 bytes terminados en NUL), dentro de la NCA de control.

Implementado en M2: se localiza la NCA de control del CNMT, se deriva su clave, se lee su
RomFS y se extrae únicamente `control.nacp` (sin volcar el resto). `info` y `dump` muestran
`v1.0.0`/`v1.1.0` y siguen mostrando `meta v0`/`meta v131072` como dato interno. La versión
visible **nunca** se deduce del nombre del archivo.

**Cabeceras extendidas que usamos:**

| Tipo | Campos útiles |
|---|---|
| Application | `patch_id`, `required_system_version`, `required_application_version` |
| Patch | `application_id`, `required_system_version` |
| AddOnContent | `application_id`, `required_application_version` |

`patch.application_id` es la forma **fiable** de emparejar un update con su base (mejor que restar
`0x800` al title id, aunque ambas deben coincidir).

### 21.2 Ticket

```
0x000  signature data     u32 tipo (LE) + firma + padding a 0x40   → tamaño Y
Y+0x00 Issuer             0x40
Y+0x40 TitleKeyBlock      0x100   ← bloque cifrado (o mensaje RSA)
Y+0x140 TicketVersion     u8  (2)
Y+0x141 TitleKeyType      u8  (0 = common/AES, 1 = personalized/RSA)
Y+0x142 TicketVersion     u16
Y+0x144 LicenseType       u8
Y+0x145 MasterKeyRevision u8
Y+0x148 Reserved          0x08
Y+0x150 TicketID          0x08
Y+0x158 DeviceID          0x08
Y+0x160 RightsID          0x10
Y+0x170 AccountID         0x04
```

- El **tipo de firma va en little-endian** en disco (`0x010004` se lee `04 00 01 00`). Si
  se interpreta como BE sale `0x04000100`, que no corresponde a ninguna firma conocida. El
  parser acepta BE como último recurso, pero el orden real es LE.
- Firma habitual: **RSA-2048/SHA-256 (`0x010004`)** → `Y = 0x140` → bloque en **`0x180`**.
- RSA-4096 → `Y = 0x240` → bloque en `0x280`; ECDSA → `Y = 0x80` → bloque en `0xC0`.
- `TitleKeyType = 0`: el bloque se descifra con `titlekek[rev]` (AES-ECB).
- `TitleKeyType = 1`: está cifrado con la clave RSA de una consola concreta → **no soportado**,
  error explícito.

### 21.3 XCI

```
0x000  header_sig        0x100
0x100  magic             "HEAD"
0x104  secure_offset     u32
0x10D  cart_type         u8
0x118  cart_size         u64
0x120  reversed_iv       0x10
0x130  hfs0_offset       u64
0x138  hfs0_header_size  u64
0x140  hfs0_header_hash  0x20
0x160  crypto_header_hash 0x20
0x190  encrypted_data    0x70
```

- El **root HFS0** se localiza con `hfs0_offset` y su nombre/tamaño están en su propia cabecera.
- Particiones habituales: `secure` (contenido del juego), `update` (parche) y `normal`.
- En cartucho las NCA suelen no tener rights id y usar **key area**; el orden de derivación de §8.1
  ya lo contempla.
- Un cartucho puede no incluir tickets (`.tik`). Si una NCA de programa tiene rights id y no hay
  ticket en el contenedor, la clave debe aportarse en `title.keys`.
- `secure` puede contener varios CNMT (base + update en un XCI "con update"); se leen todos y
  se clasifican por su `ContentMetaType`.
- XCI recortados (sin `update`) son válidos: el update simplemente no existe; las particiones
  de tamaño cero se ignoran.

### 21.4 DLC

- `ContentMetaType = AddOnContent`; su title id es `base + 0x1000 + n`.
- Puede tener update propio (`Patch` sobre el DLC, con su CNMT `DataPatch` en firmwares recientes).
- Salida: siempre en subcarpeta separada, nunca mezclado con el romfs del juego.

```
salida/
  exefs/  romfs/            (juego base + update)
  dlc/<dlc_title_id>/romfs/ (cada DLC)
```

### 21.5 Particiones extra

Una NCA Program puede contener, además de exefs y romfs, una partición `logo`. Por defecto se
ignora; `--all-parts` la extrae como `logo/` junto a `exefs/` y `romfs/`.

### 21.6 Selección de base y de update

| Situación | Regla |
|---|---|
| Una base | se usa |
| Varias bases | la de versión más alta ≤ update; si no hay update, la más alta |
| Un update | se usa |
| Varios updates | el más alto; los demás se ignoran con un aviso, salvo que `--version` elija uno |
| `--version` de la base | se extrae la base sin update |
| `--version` inexistente | error `E_VERSION_NOT_FOUND` con la lista de versiones disponibles |
| Update sin base | error 3 (un futuro `--delta` extraerá solo los bloques de parche) |
| Base con title id que no empareja | error: `patch.application_id` no coincide |

### 21.7 Agrupación

La agrupación es por `title_id` del CNMT, no por nombre de archivo:

- Base: `ContentMetaType = Application`.
- Update: `Patch`, con `patch.application_id` igual al title id de la base.
- DLC: `AddOnContent`, todos bajo el `application_id` del juego.

Un NSP puede contener varios contenidos (base + DLC a la vez): el motor los detecta, `batch`
los agrupa por aplicación y `dump` los extrae todos en una sola invocación cuando se usa
`--tree`. Sin `--tree`, un archivo combinado se rechaza con un error explícito para no
mezclar dos títulos en la misma carpeta. En el layout de árbol, un DLC (con o sin parche)
va siempre a `<app_id>/dlc/<dlc_id>/v<versión>`. Al extraer un título solo se abren las NCA
de ese título y de sus parches: un DLC sin `title.keys` no impide extraer la base.

---

## 22. Errores y verificación

### 22.1 Catálogo de errores

Formato estable en `--json`:

```json
{
  "error": {
    "code": "E_KEY_TITLEKEK_MISSING",
    "message": "falta titlekek_09",
    "hint": "tu keyset llega hasta la revisión 8; el update necesita la 9",
    "context": { "required_revision": 9, "crypto_type": 10, "file": "update.nsp" }
  }
}
```

| Código | Significado |
|---|---|
| `E_IO` | error de lectura/escritura |
| `E_KEY_TITLEKEK_MISSING` | falta `titlekek_NN` |
| `E_KEY_TITLEKEY_MISSING` | rights id sin ticket ni entrada en `title.keys` |
| `E_KEY_HEADER_MISSING` | falta `header_key` |
| `E_KEY_AREA_MISSING` | falta `key_area_key_*` |
| `E_KEYSET_INVALID` | el archivo de claves no se puede interpretar |
| `E_BASE_MISSING` | update sin base |
| `E_BASE_MISMATCH` | la base no corresponde al update |
| `E_FORMAT_UNSUPPORTED` | NSZ/NCZ, NCA desconocida |
| `E_LAYOUT_UNEXPECTED` | sección sin tablas BKTR o IVFC ilegible |
| `E_OUTPUT_EXISTS` | ya existe el destino y no se pasó `--force` |
| `E_NAME_COLLISION` | colisión de nombres en el destino |
| `E_NO_SPACE` | espacio insuficiente |
| `E_VERIFY_FAILED` | hash o tamaño no coinciden |
| `E_VERSION_NOT_FOUND` | `--version` no existe entre la base y los updates disponibles |
| `E_TICKET_PERSONALIZED` | titlekey cifrado con RSA de consola |
| `E_CANCELLED` | cancelado por el usuario |

Regla: **todo error tiene código, mensaje, pista y contexto**. Sin pista no se considera terminado.

### 22.2 Verificación

| Nivel | Qué comprueba | Por defecto |
|---|---|---|
| 1 | Tamaños y conteos del plan | sí |
| 2 | Hash maestro de IVFC | sí |
| 3 | Tablas IVFC nivel a nivel | con `--strict` o `--verify-hashes` |
| 4 | SHA-256 de cada archivo de salida vs. `SHA256SUMS` | sí (se calcula al escribir) |

`nsromexefs verify <DIR>` relee el árbol y comprueba:

- Cada ruta listada en `SHA256SUMS` existe, es un archivo normal y su SHA-256 coincide.
- Las rutas del manifiesto no escapan de la raíz (`..`, absolutas, `\`, `:`) y no se siguen
  enlaces simbólicos.
- `main` empieza por `NSO0` (si existe).
- Los archivos que no lista `SHA256SUMS` se reportan como sobrantes.
- En modo recursivo, cada `SHA256SUMS` encontrado se verifica como una raíz independiente.
- Si existe `manifest.json`, se valida `schema`, conteos y consistencia del hash/tamaño de `main`.

Por defecto, un `main` sin `NSO0` y los sobrantes son avisos; con `--strict` son fallos. Los
árboles antiguos sin manifest siguen siendo verificables por compatibilidad.

### 22.3 `--strict` frente a `--lenient`

- **Por defecto:** hash maestro obligatorio; tablas de nivel 3 y archivos sobrantes son avisos.
- **`--strict`:** cualquier discrepancia es error (código 5), incluidos sobrantes y `main` sin `NSO0`.
- **`--lenient`:** todo son avisos; útil con volcados dañados conocidos.

### 22.4 Preflight de escritura

Antes de escribir, `dump` y `batch` comprueban:

- Que el destino no sea un archivo y que, si es un directorio con contenido, se haya pasado
  `--force` (`E_OUTPUT_EXISTS`).
- Que el volumen tenga espacio para los datos planificados más un margen del 1 % + 64 MiB
  (`E_NO_SPACE`, código 7).
- Que los índices de `exefs`/`romfs` no contengan colisiones (`E_NAME_COLLISION`, código 6).
- Con `--dry-run` no se crea el destino ni se rechaza por estar ocupado; solo se calcula el
  espacio necesario.

### 22.5 Prefijo de rutas

Antes de escribir cualquier archivo se comprueba que la ruta canónica del destino es prefijo de la
ruta final. Cualquier intento de salir (`..`, ruta absoluta, enlace simbólico) aborta con `E_IO`.

---

## 23. Interfaz gráfica (v2)

### 23.1 Principio

La GUI **no implementa lógica de parsing ni extracción**: su primer corte ejecuta la CLI
`nsromexefs` como proceso local (`info --json`, `dump --dry-run --json`, `dump --json`) y consume
sus resultados/eventos de progreso. Así conserva exactamente las validaciones, cripto y fusión de
la CLI; no requiere una segunda implementación. La evolución prevista es extraer una API de
aplicación asíncrona compartida con `nsromexefs-core` cuando el núcleo exponga planificación,
cancelación y progreso como interfaz estable.

| Comando | Devuelve |
|---|---|
| `info(files, keys)` | JSON de `nsromexefs info --json` |
| `plan(files, options)` | JSON de `nsromexefs dump --dry-run --json` |
| `run(plan)` | ejecuta `dump --json`; reenvía el progreso disponible por stderr |
| `cancel()` | termina el proceso CLI, dejando los checkpoints para `--resume` |
| `keys.validate(paths)` | JSON de `nsromexefs keys --check --json` |

```rust
struct ProgressEvent {
    stage: Stage,          // Discovery | Keys | Exefs | Romfs | Verify | Done
    current_file: Option<String>,
    bytes_done: u64,
    bytes_total: u64,
    files_done: u32,
    files_total: u32,
    throughput_bps: u64,
    eta_secs: Option<u64>,
}
```

### 23.2 Stack

**Tauri 2 + Svelte 5 + TypeScript.** El primer corte de interfaz ofrece selección de NSP/XCI,
claves, destino, revisión dry-run, extracción/reanudación, resultado y ajustes del ejecutable CLI.
La lectura de progreso detallada/ETA, lotes e i18n ES/EN están pendientes. Motivos del stack:

- La app vive de tablas, chips de estado, arrastrar y soltar y barras de progreso: donde la web gana.
- El backend Rust reutiliza el crate del núcleo sin FFI ni reimplementaciones.
- WebView2 viene con Windows 10/11; el instalador lo comprueba y ofrece el runtime si falta.
- El tamaño del bundle queda en ~6-10 MB.

Alternativa considerada y descartada: **egui/eframe** (un solo toolchain, sin Node, pero mucho más
trabajo para tablas ricas y menor capacidad de pulido visual).

### 23.3 Pantallas

**1. Inicio**
- Zona de arrastre grande: "Arrastra NSP, XCI o una carpeta".
- Estado del keyset arriba a la derecha: `claves OK · hasta revisión 20` (verde) o rojo con
  "Configurar".
- Recientes y botón "Añadir archivos".

**2. Revisión** (la pantalla clave)
- Tabla agrupada por título: `Juego · base v1.0.0 · update v1.1.0 · DLC (2)`.
- Chip por grupo: **Fusionar** / **Solo base** / **Falta base** / **Solo delta**.
- Selector de versión si hay varios updates.
- Panel derecho: destino, espacio requerido vs libre, conflictos, resumen
  ("3.836 archivos · 6,23 GB · ~45 s").
- Botón "Extraer" deshabilitado con el motivo visible si falta algo.

**3. Progreso**
- Barra global + fase: `romfs · 45% · 2,8/6,2 GB · 180 MB/s · ETA 12 s`.
- Log colapsable con niveles; botón Cancelar que deja estado reanudable.
- Sin diálogos modales bloqueantes.

**4. Resultado**
- "Listo: 3.836 archivos · 6,23 GB · versión 1.1.0" y checks OK.
- Acciones: **Abrir carpeta**, Copiar ruta, Ver manifest, Exportar log.
- No hay acciones sobre otros programas.

**5. Ajustes**
- Archivos de claves (selector + arrastrar), jobs, idioma (ES/EN), tema claro/oscuro/sistema,
  destino por defecto.

**Acerca de**: versión, licencia MIT, y los créditos de `THIRD-PARTY-NOTICES.md`.

### 23.4 Detalles de UX

- Errores en lenguaje claro **con la acción siguiente**, usando el `hint` del núcleo.
- Cancelar siempre deja estado reanudable.
- Copiar cualquier error al portapapeles.
- Atajos: `Ctrl+O` añadir, `Ctrl+Enter` extraer, `Esc` cancelar.
- Foco visible y navegación completa por teclado.
- i18n desde el primer día con archivos de traducción (`es`, `en`).

### 23.5 Riesgos

| Riesgo | Mitigación |
|---|---|
| Dependencia de WebView2 | comprobación en el instalador y enlace al runtime |
| Dos toolchains (Node + Rust) | CI construye ambos; versiones fijadas |
| Divergencia CLI/GUI | la GUI solo llama al núcleo; cero lógica propia |
| Coste | GUI en v2, después de validar el motor |

---

## 24. Stack y empaquetado

### 24.1 Por qué Rust

| Criterio | Rust | C# | C++ | Go | Python |
|---|---|---|---|---|---|
| Binario único | ✅ 2-4 MB | ⚠️ 15-20 MB | ✅ | ✅ | ❌ |
| AES con aceleración | ✅ `aes` | ⚠️ CTR/XTS manuales | ✅ OpenSSL | ✅ | ⚠️ |
| CTR / XTS / CTR-EX | ✅ crates listos | ⚠️ a mano | ✅ a mano | ⚠️ | ⚠️ |
| Parser ante input hostil | ✅ + fuzzing | ✅ | ❌ | ✅ | ✅ |
| GUI | ✅ Tauri | ✅ Avalonia | ✅ Qt | ⚠️ | ⚠️ |

Razones decisivas: criptografía resuelta, binario pequeño y seguridad de memoria en el parser.

### 24.2 Workspace

```
nsromexefs/
  Cargo.toml            (workspace)
  crates/
    nsromexefs-core/    lib: todo el motor, sin I/O de terminal
    nsromexefs-cli/     bin: clap, salida humana/JSON
    nsromexefs-gui/     (v2) tauri
  docs/
    keys.md             guía de uso y manejo seguro de claves
    design.es.md        este documento
  tests/
    fixtures/           corpus dorado (manifiestos de hash, no el contenido)
```

### 24.3 Distribución

| Canal | Contenido |
|---|---|
| Windows | `nsromexefs-x86_64-pc-windows-msvc.zip` (portable) + instaladores MSI y NSIS (GUI, sin firmar) |
| Linux | `nsromexefs-x86_64-unknown-linux-gnu.tar.gz` (portable; requiere glibc compatible) |
| macOS | `aarch64-apple-darwin` y `x86_64-apple-darwin` (tar.gz) |

Los instaladores de GUI se distribuyen solo para Windows. En Linux y macOS se usa el código
fuente; el proyecto no publica binarios de GUI para esas plataformas. Tampoco se publica en
crates.io: el único canal oficial son las GitHub Releases.

- El workflow de tag `v*` valida que el tag coincida con la versión Cargo, compila las cuatro
  variantes de CLI y los instaladores MSI/NSIS de Windows, y crea un borrador de GitHub Release
  con `SHA256SUMS`; una persona revisa y publica.
- CI: fmt + Clippy + tests + build release en cada PR/push, en Windows, Linux y macOS.
- La firma de código no está prevista. El fuzzing se ejecuta en un workflow aparte; el workflow
  de release crea un borrador y la publicación es manual.
- Sin auto-actualización en v1.

### 24.4 Configuración

`nsromexefs.toml` (opcional, junto al ejecutable o en el directorio de configuración):

```toml
keys = ["prod.keys", "title.keys"]
jobs = 6
out = "."
strict = false
```

Precedencia: CLI > entorno > `nsromexefs.toml` > valores por defecto.

---

## 25. Licencia y atribución

### 25.1 Licencia

**MIT** para todo el proyecto (`LICENSE`).

Motivos: es la misma familia que las fuentes de las que se porta lógica (hactool ISC, nstool MIT),
es la habitual en proyectos de recompilación y no impone obligaciones de propagación de `NOTICE`
como Apache-2.0. Si en el futuro se necesita protección de patentes, se puede pasar a
`MIT OR Apache-2.0`.

### 25.2 Atribución

`THIRD-PARTY-NOTICES.md` contiene los textos íntegros y qué se tomó de cada fuente:

| Fuente | Licencia | Qué se usa |
|---|---|---|
| hactool (SciresM) | ISC | estructura NCA, CTR-EX, BKTR, romfs/IVFC, XCI |
| nstool (jakcron) | MIT | referencia de layout y validación cruzada |
| libnintendo-hac (jakcron) | MIT | estructuras CNMT y ticket |
| switchbrew | documentación | formatos públicos |
| RustCrypto | MIT/Apache-2.0 | `aes`, `ctr`, `sha2` |

Además:

- Cabecera de atribución en los módulos que porten lógica concreta (`crypto/xts.rs`, `bktr.rs`,
  `cnmt.rs`, `ticket.rs`).
- Pantalla de créditos en **Ajustes** de la GUI, y `--version` / `--help` en la CLI.
- `Cargo.lock` y `package-lock.json` registran el grafo completo de dependencias.

### 25.3 Marcas

El nombre no incluye "Nintendo" ni "Switch". En la documentación se describe compatibilidad, no
afiliación. Aviso explícito en el README.

---

## 26. ADRs y plan revisado

### ADR-01 — Rust para núcleo y CLI

**Decisión:** Rust 2021, workspace con `core` y `cli` separados.
**Motivo:** cripto lista, binario pequeño, seguridad de memoria, fuzzing.
**Consecuencia:** curva de aprendizaje mayor que C#; se compensa con la calidad del ecosistema.

### ADR-02 — Streaming por secciones con `Read + Seek` virtual

**Decisión:** toda lectura pasa por un lector de sección que abstrae CTR/CTR-EX/BKTR.
**Motivo:** permite fusionar sin escribir NCA descifradas y compartir código entre base y update.
**Consecuencia:** hay que resolver el mapeo virtual→físico una sola vez, bien.

### ADR-03 — No replicar las validaciones de layout de hactool

**Decisión:** aceptar lo que el archivo declare, con límites de seguridad.
**Motivo:** el caso real de Link's Awakening es rechazado por hactool por dos anomalías que la
consola acepta (Anexo B).
**Consecuencia:** hay que probar con más archivos reales; el fuzzing cubre la parte de seguridad.

### ADR-04 — Sin claves embebidas

**Decisión:** el keyset es siempre externo.
**Motivo:** legalidad y mantenibilidad.
**Consecuencia:** el usuario debe aportar claves; de ahí §18 y `keys.md`.

### ADR-05 — Salida plana por defecto

**Decisión:** `-o DIR` produce `DIR/exefs` y `DIR/romfs`; `--tree` para lote.
**Motivo:** es lo que consumen los proyectos de recompilación, sin pasos intermedios.
**Consecuencia:** el modo lote obliga a `--tree`.

### ADR-06 — Escritura atómica con journal

**Decisión:** `.nrx-tmp` + rename, y `manifest.json` como marca de completitud.
**Motivo:** un corte no debe dejar un árbol aparentemente válido.
**Consecuencia:** coste pequeño en I/O; ganancia enorme en confianza.

### ADR-07 — GUI en v2 con Tauri

**Decisión:** no bloquear el motor con la interfaz; Tauri 2 + Svelte después.
**Motivo:** validar primero el núcleo contra golden tests.
**Consecuencia:** la CLI debe ser completa y usable por sí sola (lo es).

### Plan revisado

| Fase | Contenido | Días |
|---|---|---|
| Documento | cerrado | — |
| CLI | M0-M6 + §18-§22 | ~8 |
| GUI | §23 | ~5-6 |
| **Total** | | **~13-14** |

---

## Anexo A — Estructuras

```c
// Entrada de sección (cabecera NCA)
struct section_entry { u32 media_start; u32 media_end; u8 pad[8]; };   // ×0x200

// Cabecera de sección
struct fs_header {
  u8  _0x0, _0x1;
  u8  partition_type;   // 0 RomFS, 1 PFS0
  u8  fs_type;          // 2 PFS0, 3 RomFS
  u8  crypt_type;       // 1 none, 2 XTS, 3 CTR, 4 BKTR
  u8  _0x5[3];
  u8  superblock[0x138];
  u8  section_ctr[8];
  u8  _0x148[0xB8];
};

// IVFC (0xE0)
struct ivfc_hdr {
  u32 magic;            // "IVFC"
  u32 id;
  u32 master_hash_size;
  u32 num_levels;       // 6
  struct { u64 logical_offset; u64 hash_data_size; u32 block_size_log2; u32 rsv; } levels[6];
  u8  pad[0x20];
  u8  master_hash[0x20];
};

// BKTR superbloque (0x138)
struct bktr_superblock {
  ivfc_hdr ivfc;        // 0xE0
  u8  pad[0x18];
  bktr_hdr reloc;       // 0xF8
  bktr_hdr subsec;      // 0x118
};

struct bktr_hdr { u64 offset; u64 size; u32 magic; u32 version; u32 num_entries; u32 rsv; }; // 0x20

struct reloc_entry { u64 virt_offset; u64 phys_offset; u32 is_patch; };   // 0x14

struct subsec_entry { u64 phys_offset; u32 rsv; u32 ctr_val; };           // 0x10

// Bloque de relocalización: cabecera 0x4000 + buckets de 0x4000
// Bucket reloc: u32 pad; u32 num; u64 virtual_offset_end; reloc_entry[818]; u8 pad[8];
// Bloque de subsecciones: cabecera 0x4000 + buckets de 0x4000
// Bucket subsec: u32 pad; u32 num; u64 physical_offset_end; subsec_entry[1023];

// CNMT (ver §21.1)
struct sContentMetaHeader { u64 id; u32 version; u8 type; u8 r0; u16 exhdr_size;
                            u16 content_count; u16 content_meta_count; u8 attributes;
                            u8 storage_id; u8 install_type; u8 install_state;
                            u32 required_download_system_version; u8 r1[4]; }; // 0x20
struct sContentInfo { u8 hash[0x20]; u8 content_id[0x10]; u32 size_lower;
                      u16 size_higher; u8 content_type; u8 id_offset; };       // 0x38
struct sContentMetaInfo { u64 id; u32 version; u8 type; u8 attributes; u8 r[2]; }; // 0x10
```

---

## Anexo B — Caso real: *Link's Awakening* 1.1.0

NCA del update (`88d7f50b596192d406c20e6c395dfb80.nca`, 37.976.576 B, `crypto_type` 10 →
**`titlekek_09`**):

- Sección 1 = `Patch RomFS`, `section_ctr = 0000000200000001`, inicio `0x4000`.
- Superbloque válido: `reloc.offset = 0x8F3800`, `subsec.offset = 0x8FB800`, ambos de `0x8000`.
- `reloc.total_size = 0x17433AC00` = **6.232.003.903 B** → tamaño del romfs virtual.
- **Anomalía 1:** la entrada de sección declaraba `media_end = 0x485D` → `0x907A00` B, pero las
  tablas terminan en `0x903800` (`0x4200` de más). hactool aborta por comparar el tamaño de sección
  con el final de las tablas.
- **Anomalía 2:** `subsec.total_size = 0x8F3800` (offset de la tabla de relocalización) en lugar de
  `0x8FB800`. hactool lo compara con `subsec.offset`.
- Resultado de la fusión: 3.836 archivos / 6.232.003.903 B; única diferencia con 1.0.0:
  `region_common\event\SinkingSword.bfevfl` (2.712 → 2.920 B).
- **Validación cruzada:** nstool 1.9.2, con `title.keys` en `%USERPROFILE%\.switch\`, produjo los
  mismos 3.836 archivos con SHA256 idéntico. `nsromexefs` reproduce exactamente eso:
  3.842/3.842 archivos idénticos (exefs + romfs), 0 distintos.
- **Anomalía 3 (layout de tablas):** los buckets empiezan en `0x4000` fijo, no detrás del array
  de `num_buckets` offsets, que es de tamaño fijo (0x3FF0). Ver §5.5.

Datos de la base para los golden tests: `main` 14.984.660 B (1.0.0) y 14.983.439 B (1.1.0),
`SinkingSword.bfevfl` 2.712 B (1.0.0) y 2.920 B (1.1.0).

---

## Anexo C — Equivalencias con las herramientas actuales

| `nsromexefs` | hactool 1.4.0 | nstool 1.9.2 |
|---|---|---|
| apertura de NSP | `-t pfs0 --pfs0dir` | `-x` / `--fstree` |
| content key | `--contentkey=<k>` (calculada a mano) | `--titlekey=<tk>` o `title.keys` en `~/.switch` |
| exefs | `-t nca --exefsdir` | `--part0` |
| romfs base | `-t nca --romfsdir` | `--part1` |
| fusión update | `--basenca` (exige NCA base **descifrada** y rechaza layouts no canónicos) | `--basenca` (exige `title.keys` en `~/.switch`) |
| NCA descifrada | `--plaintext` (6,2 GB) | no soportado |
| verificación | `-y` | `--verify` |

Lecciones incorporadas al diseño:

1. No replicar las validaciones de layout de hactool: son más estrictas que el formato real.
2. No exigir rutas ocultas ni formatos alternativos: leer los archivos de claves del usuario, donde
   estén, y aceptar `prod.keys` + `title.keys`.
3. No escribir NCA descifradas de 6 GB: el `--basenca` de hactool obliga a ello; aquí el base se
   descifra al vuelo.
4. Comparar siempre contra una segunda implementación (golden test) antes de dar un volcado por
   bueno.

---

## Anexo D — Glosario

- **NCA**: Nintendo Content Archive; contenedor de una sección de contenido.
- **NSP**: paquete de contenido (PFS0) con tickets, CNMT y NCAs.
- **XCI**: imagen de cartucho (HFS0) con particiones secure/update/normal.
- **CNMT**: metadatos del título (versión, tipo, lista de contenidos).
- **BKTR**: formato de parche diferencial usado por los updates.
- **CTR-EX**: CTR con contador por bloque y mapeo físico/virtual.
- **IVFC**: árbol de hashes por niveles que envuelve el RomFS.
- **KAEK**: key area encryption key; descifra el key area de la NCA.
- **rights id**: identificador que enlaza una NCA con su ticket.
- **titlekey**: clave de contenido; va cifrada en el ticket o descifrada en `title.keys`.
- **revisión de master key**: índice de `titlekek`/`master_key`, calculado como
  `max(crypto_type, crypto_type2) - 1`.

---

## Anexo E — Convenciones de nombres

| Elemento | Valor |
|---|---|
| Repositorio | `nsromexefs` |
| Binario | `nsromexefs` (siempre minúsculas) |
| Crates | `nsromexefs-core`, `nsromexefs-cli`, `nsromexefs-gui` |
| Variable de entorno | `NSROMEXEFS_KEYS` |
| Configuración | `nsromexefs.toml` |
| Journal | `.nsromexefs-journal` |
| Temporales | `*.nrx-tmp` |
| Manifest | `manifest.json` (`schema: 1`) |
| Hashes | `SHA256SUMS` |
| Subcomandos | `info`, `dump`, `verify`, `batch`, `keys` |

En prosa puede escribirse **NSromexefs** como nombre propio; en todo lo ejecutable va en minúsculas.

---

*Fin del documento. Versión 1.1 — 2026-10-27.*
