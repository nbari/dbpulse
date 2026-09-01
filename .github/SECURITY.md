# Security Policy

## Supported Versions

Security updates are provided for the latest published `dbpulse` release line.
Users should upgrade to the newest patch release before reporting a problem.

| Version | Supported |
| ------- | --------- |
| 0.9.x   | ✅        |
| < 0.9   | ❌        |

## Reporting a Vulnerability

Please do not open a public GitHub issue or discussion for a suspected
security vulnerability.

Report vulnerabilities privately by emailing
[nbari@tequila.io](mailto:nbari@tequila.io). Include, when possible:

- A description of the vulnerability and its potential impact
- The affected `dbpulse` version, platform, and database backend
  (MySQL/MariaDB or PostgreSQL)
- Steps to reproduce the issue or a minimal proof of concept
- Whether the issue involves DSN parsing or credential handling, the
  MySQL/PostgreSQL query paths, TLS configuration and certificate
  validation, or the HTTP metrics/health endpoint
- Any suggested mitigation or fix

You can expect an initial response within 48 hours and a status update within
seven days. If the report is accepted, the maintainer will coordinate a fix
and release timeline based on its severity and complexity. If it is declined,
the response will explain why it is not considered a vulnerability.

Please keep the report confidential until a fixed release is available or a
disclosure timeline has been agreed upon.

## Scope

Reports about vulnerabilities in `dbpulse` itself or in the way it uses its
dependencies are in scope. Examples of in-scope issues:

- Leaking database credentials from a DSN into logs, metrics, or error output
- Accepting an invalid, expired, or untrusted certificate when TLS is enabled,
  or silently downgrading a connection that was requested as encrypted
- Exposing sensitive information through the Prometheus metrics endpoint
- Unsafe construction of the probe SQL statements

Out of scope:

- General support questions and configuration help
- The metrics/health endpoint being reachable by anyone who can reach the
  port it binds to; `dbpulse` does not authenticate that endpoint, so it is
  expected to be bound to a trusted network or placed behind a proxy
- Vulnerabilities that only affect an upstream dependency should be reported
  to the relevant upstream project, unless `dbpulse` uses the dependency in an
  exploitable way
