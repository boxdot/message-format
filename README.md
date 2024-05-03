# Message Format

Port of the [`MessageFormat`] class from the internalization Dart package `intl`
to Rust. The port is verbatim. In particular, memory model, error handling and
optimizations are not Rust idiomatic.

The source ported: https://github.com/dart-lang/i18n/blob/98e7b4aea2e6ff613ec273ca29f58938d9c5b23d/pkgs/intl/lib/message_format.dart

[`MessageFormat`]: https://pub.dev/documentation/intl/latest/message_format/MessageFormat-class.html
