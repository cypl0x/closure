// The Dart half of the ABI contract.
//
// `crates/closure-ffi/tests/abi_contract.rs` proves the Rust side keeps
// its promises. It cannot prove that *these* bindings describe the same
// functions: a wrong `Uint32` where the header says `size_t`, a missed
// `free`, a version check nobody wired up. Those only show up from
// here, against the real .so.
//
// So this is not a UI test. It is the other end of the same contract,
// and it is why the Dart side is worth having tests at all given it
// sits outside the hermetic gate (I10).

import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:closure_shell/closure_ffi.dart';

/// A vault the tests own, written fresh so nothing depends on the
/// developer's real notes.
Directory vaultDir() {
  final d = Directory.systemTemp.createTempSync('closure_dart_');
  File('${d.path}/notes.org').writeAsStringSync('''
* First headline
:PROPERTIES:
:ID: 01DARTFFI000000000001
:END:
Body of the first.
* Second headline
:PROPERTIES:
:ID: 01DARTFFI000000000002
:END:
Body of the second.
''');
  return d;
}

void main() {
  test('the library and these bindings agree on the ABI version', () {
    // The check that must come first. A .so built from a different
    // commit is the failure that corrupts rather than errors, and it is
    // exactly the failure a hand-written binding invites.
    expect(Closure.instance.abiVersion, equals(Closure.expectedAbiVersion));
  });

  test('a vault opens and its headlines come back in order', () {
    final d = vaultDir();
    final s = ClosureSession.open(d.path);
    expect(s, isNotNull);
    expect(s!.rowCount, equals(2));
    expect(s.rowTitle(0), equals('First headline'));
    expect(s.rowTitle(1), equals('Second headline'));
    s.close();
    d.deleteSync(recursive: true);
  });

  test('a vault that is not there is null, not an exception', () {
    expect(ClosureSession.open('/nonexistent/vault/for/a/dart/test'), isNull);
  });

  test('selecting a row gives that row its body', () {
    final d = vaultDir();
    final s = ClosureSession.open(d.path)!;
    s.select(1);
    expect(s.selectedBody, contains('Body of the second'));
    s.select(0);
    expect(s.selectedBody, contains('Body of the first'));
    s.close();
    d.deleteSync(recursive: true);
  });

  test('a row index past the end is a no-op here too', () {
    // The Rust side promises this. A binding that clamps, throws or
    // sends a negative would be a second set of rules about the same
    // question — the shape of bug this codebase keeps finding.
    final d = vaultDir();
    final s = ClosureSession.open(d.path)!;
    s.select(0);
    final before = s.selectedBody;
    s.select(99);
    expect(s.selectedBody, equals(before));
    expect(s.rowTitle(99), isNull);
    s.close();
    d.deleteSync(recursive: true);
  });

  test('a closed session refuses to be used again', () {
    // Dart has no borrow checker. Without this the natural mistake —
    // using a session after close — is a use-after-free in a UI
    // callback, which is the worst place to debug one.
    final d = vaultDir();
    final s = ClosureSession.open(d.path)!;
    s.close();
    expect(() => s.rowCount, throwsStateError);
    expect(() => s.close(), returnsNormally);
    d.deleteSync(recursive: true);
  });
  test('search narrows the rows and clearing it brings them back', () {
    final d = vaultDir();
    final s = ClosureSession.open(d.path)!;
    final all = s.rowCount;
    expect(s.search('Second'), isTrue);
    expect(s.rowCount, lessThan(all));
    expect(s.rowTitle(0), contains('Second'));
    expect(s.search(''), isTrue);
    expect(s.rowCount, equals(all));
    s.close();
    d.deleteSync(recursive: true);
  });

  test('a search matching nothing shows nothing, not everything', () {
    final d = vaultDir();
    final s = ClosureSession.open(d.path)!;
    expect(s.search('zzz-no-such-headline'), isTrue);
    expect(s.rowCount, equals(0));
    s.close();
    d.deleteSync(recursive: true);
  });

  test('capture adds a headline and a blank one is refused', () {
    final d = vaultDir();
    final s = ClosureSession.open(d.path)!;
    final before = s.rowCount;
    expect(s.capture('A thought from Dart'), isTrue);
    expect(s.rowCount, equals(before + 1));
    expect(s.capture('   '), isFalse);
    expect(s.rowCount, equals(before + 1));
    s.close();
    d.deleteSync(recursive: true);
  });
}
