# Security policy

visp-db-access controls access to production databases, so we take security
reports seriously.

## Reporting a vulnerability

Please report vulnerabilities privately through GitHub's
[private vulnerability reporting](https://github.com/djkeshawa/visp-db-access/security/advisories/new)
rather than in public issues or pull requests.

Include what you found, how to reproduce it, the affected version or commit,
and the impact you expect. You'll get an acknowledgement within a few days, and
we'll keep you updated as we investigate and fix it. Please give us a
reasonable chance to release a fix before disclosing publicly.

Examples of what's in scope: bypassing the SQL guard, approvals or grants;
reading credentials or masked data; escaping network allowlists or the target
address guard; authentication and session flaws.

## Supported versions

The project is pre-1.0. Fixes land on the `main` branch and ship in the next
release; only the latest release is supported.

## How the gateway protects databases

See [docs/SECURITY-MODEL.md](docs/SECURITY-MODEL.md) for the security model and
its known limits.
