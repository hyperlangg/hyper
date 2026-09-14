# Contributing to Hyper

Thank you for your interest in contributing to **Hyper**! We welcome contributions from the community, whether it's reporting bugs, improving documentation, or proposing new language features.

## How to Contribute

### 1. Reporting Bugs & Feature Ideas
* Before creating a new issue, check the existing [Issues](https://github.com/Yusupov-Muhammadyusuf/hyperlang/issues) to avoid duplicates.
* When reporting a bug, please include:
  * Your operating system and environment.
  * Clear steps to reproduce the issue.
  * Expected vs. actual behavior.

### 2. Code Contributions
If you want to add code or fix bugs:
1. **Fork** the repository.
2. Make your changes and commit them with clear, concise messages using standard prefixes (e.g., `feat:`, `fix:`, `docs:`).
3. Push to your fork and submit a **Pull Request (PR)** to the `main` branch.
4. After the PR is open, **run and check CI** (see below). A PR is not ready until that check is green.

### 3. Check CI after you open the PR

Do not stop at “PR opened.” The **CI** workflow is how we check that the PR and the code are actually correct. Many PRs sit with CI never run or still failing — those are not reviewable.

CI starts automatically on pull requests to `main` (workflow name: **CI**). After you open the PR, or after you push more commits:

1. Open the PR’s **Checks** tab and wait until the **CI** workflow has **run** on the latest commit. Queued, skipped, or “waiting for approval” does not count as a check.
2. If CI fails, fix the code and push again. Wait for the new run. Do not ask for review while checks are red or have not finished.
3. First-time contributors: GitHub may hold the workflow until a maintainer approves it. Until that run finishes green, the PR has not been verified.

A green **CI** result on the latest commit is required before the PR is ready for review.

## Commit Conventions

We follow standard commit message conventions to keep our git history clean, readable, and easy to parse automatically.

For guidelines on commit messages and history management, see [doc/COMMIT_CONVENTION.md](doc/COMMIT_CONVENTION.md).

## Code Guidelines

* **Commit Messages:** Contributors must create commit messages with a [prefix](doc/COMMIT_CONVENTION.md) (e.g., `feat:`, `fix:`, `docs:`, `ci:`). Prefer **one commit per file** (with limited exceptions) — see [One commit per file](doc/COMMIT_CONVENTION.md#one-commit-per-file).
* **Readability First:** Prefer clear code in the compiler pipeline (`lowering` / `codegen` / runtime).
* **Testing:** Add or extend compile-path CI smokes (`run` / `compile` / `--emit-exe`) for behavior changes.

> **Note:** Large architectural changes or major syntax modifications should be discussed in an [issue](https://github.com/Yusupov-Muhammadyusuf/hyperlang/issues) before submitting a PR. 