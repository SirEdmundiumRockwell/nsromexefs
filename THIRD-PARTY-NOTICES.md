# Créditos y licencias de terceros

`nsromexefs` es una reimplementación de formatos documentados públicamente. Este archivo
reconoce el trabajo de los proyectos en los que se basa y reproduce las licencias aplicables.

Si eres autor de alguno de estos proyectos y crees que la atribución es incorrecta o incompleta,
abre un *issue* y se corrige.

---

## hactool

- **Autor:** SciresM
- **Repositorio:** https://github.com/SciresM/hactool
- **Licencia:** ISC
- **Qué se usa en `nsromexefs`:** estructura de NCA y sus secciones, modo CTR y CTR-EX,
  interpretación de las tablas BKTR, RomFS/IVFC y cabecera XCI.

```
ISC License

Copyright (c) 2018, SciresM

Permission to use, copy, modify, and/or distribute this software for any
purpose with or without fee is hereby granted, provided that the above
copyright notice and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
```

---

## nstool

- **Autor:** jakcron (Jack)
- **Repositorio:** https://github.com/jakcron/nstool
- **Licencia:** MIT
- **Qué se usa en `nsromexefs`:** referencia de layout de NCAs y validación cruzada de la
  extracción (el caso de prueba de *Link's Awakening* 1.1.0 se comparó contra nstool 1.9.2).

```
MIT License

Copyright (c) 2019 Jack

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

---

## libnintendo-hac

- **Autor:** jakcron (Jack)
- **Repositorio:** https://github.com/jakcron/libnintendo-hac
- **Licencia:** MIT
- **Qué se usa en `nsromexefs`:** estructuras del CNMT (`sContentMetaHeader`,
  `sContentInfo`, `sContentMetaInfo`) y del ticket, verificadas contra el formato real.

```
MIT License

Copyright (c) 2019 Jack

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

---

## switchbrew

- **Sitio:** https://switchbrew.org
- **Uso:** documentación pública de formatos (NCA, CNMT, ticket, RomFS, XCI).
- No se reproduce texto del wiki; se cita como referencia técnica.

---

## Dependencias de Rust

Las siguientes bibliotecas se enlazan en el binario. Sus textos completos se distribuyen con cada
crate y se incluyen en el inventario generado con `cargo about` en cada release.

| Crate | Licencia | Uso |
|---|---|---|
| `aes` (RustCrypto) | MIT OR Apache-2.0 | AES-128 en todos los modos |
| `ctr` (RustCrypto) | MIT OR Apache-2.0 | CTR |
| `xts-mode` (RustCrypto) | MIT OR Apache-2.0 | AES-XTS para cabeceras |
| `sha2` (RustCrypto) | MIT OR Apache-2.0 | SHA-256 |
| `cipher` (RustCrypto) | MIT OR Apache-2.0 | traits de cifrado |
| `clap` | MIT OR Apache-2.0 | CLI |
| `serde` / `serde_json` | MIT OR Apache-2.0 | manifest y JSON |
| `thiserror` | MIT OR Apache-2.0 | errores |
| `rayon` | MIT OR Apache-2.0 | paralelismo |
| `indicatif` | MIT | progreso |
| `tracing` | MIT | logs |
| `tauri` (v2, GUI) | MIT OR Apache-2.0 | ventana y puente con el núcleo |

---

## Marcas

Nintendo y Nintendo Switch son marcas de Nintendo Co., Ltd. Este proyecto no está afiliado,
patrocinado ni respaldado por Nintendo. Los nombres de juegos y consolas se usan únicamente con
fines descriptivos y de interoperabilidad.
