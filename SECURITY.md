# Security Policy

## Sensitive Data Handling

Vapourfly reads and writes Steam configuration files that may contain sensitive information. The following policies apply to all code, tests, logs, and configuration shipped with the project:

### What counts as sensitive

- Steam API keys or tokens.
- Steam account credentials (usernames, passwords, session tokens).
- Steam user IDs (SteamID64, SteamID3, account names) in non-aggregate contexts.
- Filesystem paths containing usernames or home directory names.
- Any data from `config/loginusers.vdf` or the Steam credential store.

### Rules

1. **No secrets in logs.** Vapourfly must never log API keys, tokens, session IDs, or credentials at any log level. Use redaction (e.g., `[REDACTED]`) if a value must appear in debug output.

2. **No secrets in config files.** Default configuration files and example configs must not contain real credentials. Use placeholder values like `YOUR_API_KEY_HERE`.

3. **No secrets in scan output.** CLI output (e.g., `vapourfly scan`, `vapourfly doctor`, `vapourfly diagnostics export`) must not print raw SteamIDs or account names unless the user explicitly opts in with `--verbose`.

4. **Test fixtures must be sanitized.** Any Steam configuration data committed to the repository under `data/fixtures/` must use synthetic or anonymized values. Real account data must never be committed.

5. **Backup files follow the same rules.** Vapourfly creates backups before modifying files. These backups must not be logged, uploaded, or exposed through any API.

## Vulnerability Reporting

If you discover a security vulnerability in Vapourfly, please report it responsibly.

**Do not open a public GitHub issue for security vulnerabilities.**

Instead, email: **security@vapourfly.dev**

Include:

- A description of the vulnerability.
- Steps to reproduce.
- The potential impact.
- Any suggested fix (optional).

We will acknowledge your report within 72 hours and aim to ship a fix within 14 days for confirmed vulnerabilities.

### Scope

The following are in scope:

- Credential or token leakage through logs, output, or error messages.
- Path traversal or symlink attacks via crafted VDF files.
- Arbitrary file write through collection or config manipulation.
- Denial of service via malformed input files.

The following are out of scope:

- Vulnerabilities in Steam itself or Valve's infrastructure.
- Social engineering attacks against Vapourfly users.
- Issues in third-party dependencies (report these upstream).

## Supported Versions

Security fixes are applied to the current `main` branch and the latest release tag. Older versions are not supported.
