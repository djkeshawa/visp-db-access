# Contributing

Thanks for your interest in visp-db-access. Bug reports, fixes, documentation
improvements and feature proposals are all welcome.

## Before you start

- For anything beyond a small fix, open an issue first to discuss the change.
  This tool guards production databases, so changes to the SQL guard, access
  model, approvals or credential handling get careful review.
- Report security vulnerabilities privately; see [SECURITY.md](SECURITY.md).
  Don't open public issues for them.

## Making a change

1. Set up your environment with [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md).
2. Keep changes focused, and add tests for new behavior and for bug fixes.
   Guard changes need tests for both dialects and for bypass attempts.
3. Update `docs/API.md` together with any API change, and the relevant docs for
   anything user-visible.
4. Make sure the checks pass:

   ```sh
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets --all-features -- -D warnings
   cargo test --workspace --all-features
   cd web && npm run typecheck && npm run lint && npm run format:check && npm test && npm run test:e2e && npm run build
   ```

5. Open a pull request describing what changed and why, and how you tested it.

## Guidelines

- Rust: no `unsafe`, and no `unwrap`, `expect` or `panic!` outside tests.
  Prefer failing closed: when in doubt, deny.
- UI: follow the [design system](docs/DESIGN.md); new screens must pass the
  axe scan in both themes at desktop and phone sizes.
- Write commit messages in the imperative mood ("Add …", "Fix …").

## License

By contributing, you agree that your contributions are licensed under the
[Apache License 2.0](LICENSE), the license of this project.
