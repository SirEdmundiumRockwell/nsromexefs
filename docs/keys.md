# Key file guide

`nsromexefs` does not distribute keys or content. Use only dumps and keys obtained from your
own console and from content you are entitled to use. Do not share or upload your key files.

## Which files are needed

- **`prod.keys`**: contains the system keys used to decrypt headers and key areas. It is enough
  for many XCI cartridges.
- **`title.keys`**: may be required for NSPs whose NCAs depend on tickets that are not present in
  the container, such as some updates or ticketless dumps.

Pass both files with the repeatable `-k` option:

```console
nsromexefs info game.nsp -k prod.keys -k title.keys
nsromexefs dump game.nsp -o output -k prod.keys -k title.keys
```

You can also pass `-k` before the subcommand. If you omit `-k`, the tool looks for the file names
`prod.keys` and `title.keys` in its default search locations. To avoid ambiguity about which file
was loaded, specify them explicitly.

## Checking the keyset

Before processing a container, validate the files:

```console
nsromexefs keys --check prod.keys title.keys
```

The check summarizes which key families and revisions are present and how many title keys there
are. It does not print secret values. An incomplete file can be valid in format yet lack the
revision a specific NCA needs; in that case `info` or `dump` reports which key is missing.

## Format and protection

The files use the usual `name = value` text format; comments starting with `#` and blank lines are
accepted. Keep the original names produced by your dumping tool and never paste their contents
into reports, issues, or conversations.

The repository ignores `*.keys`, `prod.keys`, and `title.keys`, but that does not replace reviewing
what you are about to publish. Keep keys out of synchronized or shared folders and protect them
like credentials.

See also `nsromexefs keys --help` for the options available in your version.
