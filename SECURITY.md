# Security Policy

## Supported Versions

| Version | Supported |
| ------- | --------- |
| main    | Yes       |

## Reporting a Vulnerability

If you discover a security vulnerability in chatixia-mesh, please report it responsibly.

**Do NOT open a public GitHub issue for security vulnerabilities.**

Instead, please email **security@chatixia.ai** with:

1. A description of the vulnerability
2. Steps to reproduce the issue
3. The potential impact
4. Any suggested fixes (optional)

We will acknowledge your report within **48 hours** and aim to provide a fix or mitigation within **7 days** for critical issues.

## Scope

This policy covers:

- The registry signaling server (`registry/`)
- The WebRTC sidecar (`sidecar/`)
- The Python agent framework (`agent/`)
- The hub monitoring dashboard (`hub/`)

## Security Measures

- **Secret scanning** and **push protection** are enabled on this repository
- **Dependabot** monitors dependencies for known vulnerabilities
- **Branch protection** requires PR reviews before merging to `main`
