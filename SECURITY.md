# Security policy

## Supported versions

| Version | Supported |
|---|---|
| 0.1.x | Yes |
| Older | No |

## Reporting a vulnerability

Please report suspected vulnerabilities **privately** through GitHub Security Advisories:

https://github.com/SirEdmundiumRockwell/nsromexefs/security/advisories/new

Do not open a public issue for a security problem. Do not attach keys, ROMs, firmware, or personal paths to the report; use redacted examples.

Include, when possible:

- the affected component (CLI, core library, GUI, or packaging);
- the version and operating system;
- a description of the impact and the conditions required to reproduce it;
- minimal reproduction steps using synthetic or redacted data.

The maintainer will acknowledge the report on a best-effort basis and coordinate disclosure after a fix is available.

## Scope

In scope:

- memory-safety or logic issues in the extraction engine;
- path traversal, unsafe file handling, or output overwrite problems;
- key handling, log redaction, or unintended network access in the GUI;
- vulnerabilities in the build or packaging workflow.

Out of scope:

- requests for keys, decryption keys, ROMs, or help obtaining copyrighted content;
- issues in third-party tools or libraries that are not caused by this project's integration;
- reports that require publishing sensitive data.

## Safe handling of keys and content

`nsromexefs` does not include or distribute keys. Treat key files as credentials: keep them out of repositories, screenshots, logs, and issue reports. The repository ignores common key and ROM file names, but that is not a substitute for reviewing what you publish.
