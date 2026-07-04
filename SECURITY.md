# Security Policy

## Supported Versions

| Version | Supported |
|---------|-----------|
| latest  | ✅         |

## Reporting a Vulnerability

If you find a security vulnerability in ccr, **do not open a public issue**.

Instead, email **joaohenriquebarbosa21@gmail.com** with:

1. Description of the vulnerability
2. Steps to reproduce
3. Potential impact
4. Suggested fix (if any)

You will receive a response within **72 hours** acknowledging receipt.

## Scope

`ccr` executes tools against the local filesystem and talks to the Anthropic API
with your credentials. Relevant vulnerabilities include:

- Path-traversal escapes from the `WorkingDir` guard (file access outside the working directory)
- Leakage of the API key / OAuth token (in logs, output, or persisted files)
- Command execution reaching outside the intended sandboxing of the `Bash` tool
- Panics or unsafe-block soundness issues reachable from model or tool input
- Dependency vulnerabilities

## Process

1. **Receipt**: acknowledgment within 72h
2. **Triage**: severity assessment
3. **Fix**: patch developed in a private branch
4. **Release**: patched version published
5. **Disclosure**: public advisory after fix is available
