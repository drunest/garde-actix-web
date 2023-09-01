# Actix-web-garde &emsp; [![Documentation]][docs.rs] [![Latest Version]][crates.io] [![Build Status]][build]


[docs.rs]: https://docs.rs/actix-web-garde/latest/actix-web-garde/
[crates.io]: https://crates.io/crates/actix-web-garde
[build]: https://github.com/rlebran-netwo/actix-web-garde/actions/workflows/build.yaml
[Documentation]: https://img.shields.io/docsrs/actix-web-garde
[Latest Version]: https://img.shields.io/crates/v/actix-web-garde.svg
[Build Status]: https://github.com/rlebran-netwo/actix-web-garde/actions/workflows/build.yaml/badge.svg?branch=main

Actix-web wrapper for [garde](https://github.com/jprochazk/garde), a Rust validation library.

- [Installation](#installation)
- [Usage example](#usage-example)
- [Feature flags](#feature-flags)

### Installation

```toml
[dependencies]
garde = "0.14"
actix-web-garde = "0.1.0"
```

### Usage example

Simply use `actix-web-garde` exposed types as a drop in for actix types.

Your types must implement `Validate` from `garde`. Validation happens during actix's `FromRequest` invocation.

If payload is invalid, a 400 error is returned (404 for Path).

Custom error handling can be implemented with an extractor config (`actix_web_garde::web::QueryConfig` in place of `actix_web::web::QueryConfig` for example).

```rust
use actix_web::HttpResponse;
// instead of actix_web::web::Path
use actix_web_garde::web::Path;
use garde::Validate;

#[derive(Validate)]
struct MyStruct<'a> {
  #[garde(ascii, length(min=3, max=25))]
  username: &'a str,
}

fn test(id: Path<MyStruct>) -> HttpResponse {
  todo!()
}
```

⚠️ When using `garde` [custom validation](https://github.com/jprochazk/garde#custom-validation), the `Context` type needs to implement `Default` which is not required by `garde`.

Context needs to be provided through actix's `data` or `app_data`, if not found default will be used instead.


### Feature flags

| name       | description                                                   | extra dependencies                                                                           |
|------------|---------------------------------------------------------------|----------------------------------------------------------------------------------------------|
| `serde_qs` | Enables the usage of `garde` for `serde_qs::actix::QsQuery<T>` | [`serde_qs`](https://crates.io/crates/serde_qs)                                      |
