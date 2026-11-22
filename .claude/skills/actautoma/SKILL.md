---
name: actautoma
description: Drive repository-hosting tools (gh, glab) through actautoma, and rename a repository everywhere with `actautoma rename-repo`. Use for a repository rename or when code needs a forge operation.
---

# actautoma

A Rust library over the hosting platforms' tools (`gh` adapter today) plus one binary.

## Command

`actautoma rename-repo OWNER/OLD NEW [--dir OLD_DIR] [--root DIR]... [--children DIR]... [--registry FILE] [--bin-dir DIR] [--into DIR] [--local-only] [--dry-run]`

Renames the repository on the platform, its origin, its folder and its written references. Run `--dry-run` first. Prefer `actrepo rename`, which calls it.

## Limits

- Code in another repository that needs a forge operation depends on this library; it never runs `gh` itself.
- A rename is a breaking change: its consumers each get a migration pull request.
