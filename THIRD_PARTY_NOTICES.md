# Third-Party Notices

## CC Switch

`cc-switchy` implements compatible snapshot restore and local Agent projection
behavior by studying and adapting portions of CC Switch.

- Project: CC Switch
- Source: https://github.com/farion1231/cc-switch
- Reference commit: `c6197ae32450cd70e2bf03b35e3f5f53ac12044c`
- License: MIT

Adapted behavior includes the `cc-switch-webdav-sync` v2 snapshot format,
WebDAV and S3 object layout, provider configuration projection for Claude,
Codex, Gemini, OpenCode, OpenClaw, Hermes, and the supported Claude Desktop
boundary. `cc-switchy` intentionally omits CC Switch's upload, deletion, proxy,
hot-switch, usage, failover, session, and Tauri runtime features.

Copyright and license terms remain with the CC Switch contributors. See the
upstream repository for its complete MIT license and contributor history.
