# Security policy

## Supported versions

The latest released version is the supported one. Inkling is a small library with a fast
release cadence; fixes land on `main` and ship in the next release rather than being
backported.

## Reporting a vulnerability

Please report privately rather than in a public issue, through
[GitHub's private vulnerability reporting](https://github.com/codizzler/inkling/security/advisories/new)
on this repository. Expect an acknowledgement within a few days.

Include what you have: the affected package and version, what an attacker gains, and a
reproduction if you have one.

## What is in scope

Inkling parses untrusted text (art files, and progress tokens on the CLI's stdin) and
writes escape sequences to a terminal. The interesting failure modes are there:

- A panic, hang, or unbounded allocation from parsing an art file or reading stdin.
- Escape sequences from art content or a caption reaching the terminal unescaped, which
  would let a text file drive the reader's terminal.
- The terminal being left in a broken state (hidden cursor, alternate screen, raw mode)
  after an abnormal exit.

Out of scope: what a program chooses to display, and the terminal emulator's own handling
of well-formed sequences.
