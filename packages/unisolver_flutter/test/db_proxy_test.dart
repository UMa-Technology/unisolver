/// Which proxy DbManager's downloads take: loopback direct, then the environment, then the system.
library;

import 'package:flutter_test/flutter_test.dart';
import 'package:unisolver_flutter/src/db_proxy.dart';
import 'package:unisolver_flutter/src/rust/api/proxy.dart';

void main() {
  final release = Uri.parse(
    'https://github.com/UMa-Technology/unisolver-data/releases/download/v3/manifest-v3.json',
  );
  final system = SystemProxy(host: '127.0.0.1', port: 7890);

  test('the environment wins over the system settings', () {
    expect(
      proxyFor(
        release,
        environment: {'https_proxy': 'env.proxy:3128'},
        system: () => system,
      ),
      'PROXY env.proxy:3128',
    );
  });

  test('without an environment proxy the system proxy is used', () {
    expect(
      proxyFor(release, environment: const {}, system: () => system),
      'PROXY 127.0.0.1:7890',
    );
  });

  test('no proxy anywhere is a direct connection', () {
    expect(
      proxyFor(release, environment: const {}, system: () => null),
      'DIRECT',
    );
  });

  test('loopback hosts go direct and never read the system settings', () {
    var asked = false;
    SystemProxy? ask() {
      asked = true;
      return system;
    }

    expect(
      proxyFor(
        Uri.parse('http://127.0.0.1:8099/manifest-v3.json'),
        environment: {'http_proxy': 'env.proxy:3128'},
        system: ask,
      ),
      'DIRECT',
    );
    expect(
      proxyFor(
        Uri.parse('http://localhost/x'),
        environment: const {},
        system: ask,
      ),
      'DIRECT',
    );
    expect(asked, isFalse);
  });
}
