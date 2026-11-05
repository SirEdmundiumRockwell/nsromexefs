# Third-Party Credits and Licenses

`nsromexefs` is an independent implementation of publicly documented formats. This file
acknowledges the projects it draws on and reproduces applicable license texts.

If you are an author of one of these projects and believe an attribution is incorrect or
incomplete, please open an issue so it can be reviewed.

---

## hactool

- **Author:** SciresM
- **Repository:** https://github.com/SciresM/hactool
- **License:** ISC
- **Use in `nsromexefs`:** NCA and section structures, CTR and CTR-EX modes, BKTR table
  interpretation, RomFS/IVFC, and XCI header references.

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

- **Author:** jakcron (Jack)
- **Repository:** https://github.com/jakcron/nstool
- **License:** MIT
- **Use in `nsromexefs`:** NCA layout reference and cross-validation of extraction
  (the *Link's Awakening* 1.1.0 test case was compared against nstool 1.9.2).

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

- **Author:** jakcron (Jack)
- **Repository:** https://github.com/jakcron/libnintendo-hac
- **License:** MIT
- **Use in `nsromexefs`:** CNMT structures (`sContentMetaHeader`, `sContentInfo`,
  `sContentMetaInfo`) and ticket structures, cross-checked against real-world formats.

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

- **Website:** https://switchbrew.org
- **Use:** public format documentation (NCA, CNMT, tickets, RomFS, XCI).
- Wiki text is not reproduced; it is cited as a technical reference.

---

## Dependencies

The project uses the following direct dependencies. Transitive dependencies and complete license
texts are recorded in `Cargo.lock` and `crates/nsromexefs-gui/package-lock.json`; each crate or
package also ships its own license text.

| Dependency | License | Use |
|---|---|---|
| `aes`, `ctr`, `sha2` (RustCrypto) | MIT OR Apache-2.0 | AES-128, CTR, and SHA-256 |
| `fs2` | MIT/Apache-2.0 | Cross-platform file locking |
| `serde` / `serde_json` | MIT OR Apache-2.0 | Manifests and JSON output |
| `thiserror` | MIT OR Apache-2.0 | Error types |
| `clap` | MIT OR Apache-2.0 | Command-line interface |
| `anyhow` | MIT OR Apache-2.0 | CLI error reporting |
| `tauri` / `tauri-plugin-dialog` | Apache-2.0 OR MIT | Desktop window, bridge, and dialogs |
| `@tauri-apps/api` / `@tauri-apps/plugin-dialog` | Apache-2.0 OR MIT | Frontend IPC and dialogs |
| `Svelte` | MIT | User interface |
| `Vite` | MIT | Frontend build tooling |
| `TypeScript` | Apache-2.0 | Frontend type checking |

---

## Trademarks

Nintendo and Nintendo Switch are trademarks of Nintendo Co., Ltd. This project is not affiliated
with, sponsored, or endorsed by Nintendo. Game and console names are used for descriptive and
interoperability purposes only.
