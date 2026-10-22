# Rust style

Follow https://google.github.io/styleguide/rustguide.html and the public Rust
style/API guidelines it references. This is a Cargo application, not Chromium
or Fuchsia; their build, import, synchronization, and test harness rules do not
apply.

- Stable Rust and rustfmt, 100 columns; lowercase hexadecimal literals.
- Leaf modules named after their primary type; root modules declare/re-export.
- No glob imports outside inline tests. Import types directly, free functions
  through their module. One source module per use statement.
- Borrow slices/str, prefer unique ownership, avoid speculative traits.
- Result for recoverable failures, useful error context, expect rather than
  unwrap for invariants. Tests include assertion context where useful.
- Document public APIs and non-obvious hardware behavior. Mark pure return
  values must_use, derive traits where appropriate, use named fields.
- Safe Rust only in first-party code; workspace forbids unsafe code.
- Run sh scripts/check.sh. Stable rustfmt cannot enforce all Google rules;
  review is still required. Do not enable nightly just for formatting.
