# Guía de claves

`nsromexefs` no distribuye claves ni contenido. Usa únicamente volcados y claves obtenidos de
tu propia consola y de contenido que tengas derecho a utilizar. No compartas ni subas tus
archivos de claves.

## Qué archivos se necesitan

- **`prod.keys`**: contiene claves de sistema para descifrar cabeceras y key areas. Es suficiente
  para muchos cartuchos XCI.
- **`title.keys`**: puede ser necesaria para NSP cuyas NCA dependen de tickets que no están
  presentes en el contenedor, por ejemplo algunos updates o volcados sin ticket.

Pasa ambos archivos con `-k` repetible:

```console
nsromexefs info juego.nsp -k prod.keys -k title.keys
nsromexefs dump juego.nsp -o salida -k prod.keys -k title.keys
```

También puedes pasar `-k` antes del subcomando. Si omites `-k`, la herramienta busca los nombres
`prod.keys` y `title.keys` en sus ubicaciones de búsqueda predeterminadas. Para evitar dudas
sobre qué archivo se cargó, indícalos explícitamente.

## Comprobar el keyset

Antes de procesar un contenedor, valida los archivos:

```console
nsromexefs keys --check prod.keys title.keys
```

La comprobación resume qué familias/revisiones están presentes y cuántas titlekeys hay. No
imprime los valores secretos. Un archivo incompleto puede ser válido como formato, pero no tener
la revisión necesaria para una NCA concreta; en ese caso `info` o `dump` informa qué clave falta.

## Formato y protección

Los archivos usan el formato de texto habitual `nombre = valor`; se aceptan comentarios que
empiezan por `#` y líneas vacías. Conserva los nombres originales producidos por tu herramienta
de volcado y no pegues los contenidos en informes, issues o conversaciones.

El repositorio ignora `*.keys`, `prod.keys` y `title.keys`, pero eso no sustituye revisar lo que
vas a publicar. Mantén las claves fuera de carpetas sincronizadas o compartidas y protégelas
como credenciales.

Consulta también `nsromexefs keys --help` para las opciones disponibles en tu versión.
