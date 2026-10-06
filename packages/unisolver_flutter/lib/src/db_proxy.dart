/// Which proxy `DbManager`'s downloads go through. Dart's `HttpClient` reads only the
/// `http_proxy` / `https_proxy` environment variables, and desktop apps started from the Dock or
/// the Start menu have none, so a proxy set in the system settings would be ignored.
library;

import 'dart:io';

import 'rust/api/proxy.dart';

/// The system's manual proxy (Rust's `systemProxy` on desktop, none on mobile, where proxies
/// arrive as a VPN). Injectable for tests.
typedef DbSystemProxy = SystemProxy? Function();

/// `HttpClient.findProxy` for [uri]: loopback hosts go direct; then the environment (Dart's own
/// rule, `no_proxy` included); then the system settings, read only when needed.
String proxyFor(
  Uri uri, {
  required Map<String, String> environment,
  required DbSystemProxy system,
}) {
  if (_loopback(uri.host)) return 'DIRECT';
  final env = HttpClient.findProxyFromEnvironment(
    uri,
    environment: environment,
  );
  if (env != 'DIRECT') return env;
  final p = system();
  return p == null ? 'DIRECT' : 'PROXY ${p.host}:${p.port}';
}

bool _loopback(String host) =>
    host == 'localhost' || host == '::1' || host.startsWith('127.');
