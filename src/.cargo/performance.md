# Performance Rules

## Memory and allocation

- Preallocate collections when size is known or estimable:
  - `String::with_capacity(estimated_len)`
  - `Vec::with_capacity(count)`
  - `BufReader::with_capacity(size, reader)`
- Prefer `&str` / `&[T]` returns over owned types when lifetime allows.
- Use `Cow<'_, str>` for conditional ownership such as `String::from_utf8_lossy`.
- Use `&'static str` for compile-time constant strings.
- Reuse buffers with `.clear()` instead of reallocating.

## Zero-cost abstractions

- Use const generics for compile-time branching such as `<const LINE_NUMBERS: bool>`.
- Use `#[inline]` on small hot-path functions.
- Prefer `core` over `std` where possible.
- Stream data instead of loading entire files when possible.

## Dependencies

- Prefer performance-oriented crates such as `parking_lot` and `memchr`.
- Keep dependency footprint minimal.
