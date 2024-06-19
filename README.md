# Message Format

[![ci-badge]][ci]

[ci-badge]: https://github.com/boxdot/message-format/actions/workflows/ci.yaml/badge.svg
[ci]: https://github.com/boxdot/message-format/actions/workflows/ci.yaml

Port of the [`MessageFormat`] class from the internalization Dart package `intl`
to Rust. The port is verbatim. In particular, memory model, error handling and
optimizations are not Rust idiomatic.

The source ported: <https://github.com/dart-lang/i18n/blob/98e7b4aea2e6ff613ec273ca29f58938d9c5b23d/pkgs/intl/lib/message_format.dart>

## License

 * Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or
   http://www.apache.org/licenses/LICENSE-2.0)
 * MIT License ([LICENSE-MIT](LICENSE-MIT) or
   http://opensource.org/licenses/MIT)

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this document by you, as defined in the Apache-2.0 license,
shall be dual licensed as above, without any additional terms or conditions.

[`MessageFormat`]: https://pub.dev/documentation/intl/latest/message_format/MessageFormat-class.html
