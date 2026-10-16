# Security policy

WinFastCleaner deletes files. A bug that deletes something it shouldn't is
treated as a security issue, not just a functional one.

## Reporting a vulnerability

**Please do not open a public issue for security problems.**

Report privately through GitHub's private vulnerability reporting:

1. Go to the repository's **Security** tab.
2. Click **Report a vulnerability**.
3. Describe the problem, the affected version or commit, and the steps to
   reproduce it. Use synthetic paths and files in your report. Never include
   real personal data, usernames, or file listings from your own machine.

You should get an acknowledgement within 7 days. Once a fix is ready, the
advisory is published with credit to the reporter, unless you'd rather stay
anonymous.

## What counts as a security issue

Report any of these privately:

- Anything that deletes, moves, or overwrites a file outside what a rule or
  the user explicitly selected, including through symlinks, junctions, mount
  points, or path tricks (`..`, short names, alternate data streams, device
  paths).
- A way to bypass the protected-paths blocklist, the exclusions list, or the
  "red tier is never automatic" rule.
- A rule file that can execute code, run a command, or target a protected
  path.
- A way to make the elevated helper (when it exists) act on paths or
  commands it wasn't asked to handle.
- Recovery bin corruption that loses files that should have been restorable.
- Any network access, telemetry, or data leaving the machine.
- Weaknesses in the Tauri configuration, such as remote content loading, CSP
  bypass, or over-broad capabilities.
- Vulnerable dependencies that are actually reachable from the app.

## Supported versions

Only the latest release, and `main`, receive security fixes until 1.0.

## How the project protects itself

- No secrets are stored in the repository. CI runs `gitleaks` on every push.
- Dependencies are checked with `cargo deny` (advisories, licenses, sources)
  and `npm audit`. Dependabot keeps Cargo, npm, and GitHub Actions current.
- GitHub Actions workflows use least-privilege `permissions:`, pin
  third-party actions to commit SHAs, and never expose secrets to pull
  requests from forks.
- Release builds are produced by CI, not on a personal machine, and each
  release publishes SHA-256 checksums.
- The deletion safety rules are enforced in the `core` library, not only in
  the UI, and are covered by automated tests. See
  [docs/safety-model.md](docs/safety-model.md).
