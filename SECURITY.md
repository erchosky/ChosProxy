# Security Policy

## Intended use

ChosProxy is for testing systems you own or have **explicit authorization**
to test (local labs such as OWASP Juice Shop, DVWA, WebGoat, PortSwigger Web
Security Academy, CTFs, development environments). Using it against systems
without permission may be illegal.

The project will not accept features whose purpose is stealth, detection
evasion, persistence, credential stuffing/password spraying, destructive
exploitation or denial of service.

## Reporting a vulnerability in ChosProxy

Please report privately (GitHub Security Advisories on this repository once
it is published). Include version, OS, reproduction steps and impact. We aim
to acknowledge within 7 days. Please do not open public issues for
vulnerabilities.

## Design commitments

* Everything is local. No telemetry, no analytics, no cloud.
* Listeners bind to loopback unless you enable remote access.
* The local CA (from Milestone 2) is generated per installation; its private
  key never leaves your machine.
* Captured content is never executed in the application's privileged
  context. See [THREAT_MODEL.md](THREAT_MODEL.md).
