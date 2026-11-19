# Contributing to Axiom (`axiom-lang`)

Thank you for your interest in contributing to **Axiom**!

## Contributor License Agreement (CLA)

Because Axiom is dual-licensed under **BUSL-1.1** (free for non-commercial use) and a **Commercial License**, all external contributors must sign our Contributor License Agreement (CLA) before a Pull Request can be merged.

By submitting a contribution under the CLA, you retain copyright to your own work while granting the Project Owner an irrevocable, worldwide, royalty-free right to distribute, sublicense, and commercialize the contribution as part of Axiom under both non-commercial and commercial terms.

## Local Quality Gate

Before opening a Pull Request, ensure all static checks and Z3 verification suites pass with zero warnings:

```bash
./scripts/run_checks.sh
```
