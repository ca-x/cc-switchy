# Third-Party Notices

## CC Switch

`cc-switchy` implements compatible snapshot restore and local Agent projection
behavior by studying and adapting portions of CC Switch.

- Project: CC Switch
- Source: https://github.com/farion1231/cc-switch
- Reference commit: `c6197ae32450cd70e2bf03b35e3f5f53ac12044c`
- License: MIT

Adapted areas include:

- the `cc-switch-webdav-sync` v2 protocol and WebDAV/S3 object layout;
- CC Switch SQLite export validation, db-v5 migration, and db-v6 restore behavior;
- provider projection for Claude, Codex, Gemini, OpenCode, OpenClaw, Hermes,
  and the supported Claude Desktop boundary;
- MCP projection and preservation of unknown local entries;
- Skills SSOT, symlink/copy, enablement, and managed-target reconciliation.

`cc-switchy` intentionally omits CC Switch's upload, deletion, proxy,
hot-switch, usage, failover, session, scheduler, provider CRUD, and Tauri
runtime features.

### Upstream MIT License

MIT License

Copyright (c) 2025 Jason Young

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
