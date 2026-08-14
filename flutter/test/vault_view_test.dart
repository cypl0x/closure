// What the window actually paints.
//
// `closure_ffi_test.dart` proves the bindings read the vault. That is
// the library, and a library test is not a shell: the gpui work in this
// repo has twice had a correct core and a pane that showed nothing,
// because the thing under test was never the thing on screen. So these
// pump the real widget over the real .so and read the text off it.

import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:closure_shell/main.dart';

Directory vaultDir() {
  final d = Directory.systemTemp.createTempSync('closure_widget_');
  File('${d.path}/notes.org').writeAsStringSync('''
* Alpha headline
:PROPERTIES:
:ID: 01WIDGET0000000000001
:END:
The body of alpha.
* Beta headline
:PROPERTIES:
:ID: 01WIDGET0000000000002
:END:
The body of beta.
''');
  return d;
}

void main() {
  testWidgets('the window shows the vault headlines', (tester) async {
    final d = vaultDir();
    await tester.pumpWidget(ClosureApp(vaultPath: d.path));
    await tester.pumpAndSettle();

    expect(find.text('Alpha headline'), findsOneWidget);
    expect(find.text('Beta headline'), findsOneWidget);

    d.deleteSync(recursive: true);
  });

  testWidgets('the first row is selected and its body is shown', (tester) async {
    final d = vaultDir();
    await tester.pumpWidget(ClosureApp(vaultPath: d.path));
    await tester.pumpAndSettle();

    expect(find.textContaining('body of alpha'), findsOneWidget);

    d.deleteSync(recursive: true);
  });

  testWidgets('tapping a headline shows that headline body', (tester) async {
    final d = vaultDir();
    await tester.pumpWidget(ClosureApp(vaultPath: d.path));
    await tester.pumpAndSettle();

    await tester.tap(find.text('Beta headline'));
    await tester.pumpAndSettle();

    expect(find.textContaining('body of beta'), findsOneWidget);
    expect(find.textContaining('body of alpha'), findsNothing);

    d.deleteSync(recursive: true);
  });

  testWidgets('a path that is not a vault says so instead of an empty window',
      (tester) async {
    // An empty outline and a broken vault look identical, and the
    // second is the one you need to be told about.
    await tester.pumpWidget(
      const ClosureApp(vaultPath: '/nonexistent/vault/for/a/widget/test'),
    );
    await tester.pumpAndSettle();
    expect(find.textContaining('Not a vault'), findsOneWidget);
  });

  testWidgets('no vault at all is a message, not a crash', (tester) async {
    await tester.pumpWidget(const ClosureApp(vaultPath: ''));
    await tester.pumpAndSettle();
    expect(find.textContaining('No vault'), findsOneWidget);
  });

  test('the palette is the one closure-shell-core declares', () {
    // `Palette` is a copy, because the ABI does not carry a theme. A
    // copy with nothing checking it is the shape of bug this codebase
    // keeps finding — one fact, two owners. This reads the Rust and
    // fails when the two drift.
    final rust =
        File('../crates/closure-shell-core/src/lib.rs').readAsStringSync();
    final start = rust.indexOf('pub const fn doom_vibrant()');
    expect(start, greaterThan(0), reason: 'doom_vibrant() moved');
    final body = rust.substring(start, start + 1400);

    final dart = File('lib/main.dart').readAsStringSync();

    for (final role in ['fg', 'bg', 'accent', 'muted', 'selection', 'heading2']) {
      final m = RegExp('$role: Color\\("#([0-9a-fA-F]{6})"\\)').firstMatch(body);
      expect(m, isNotNull, reason: 'no $role in doom_vibrant()');
      final hex = m!.group(1)!.toUpperCase();
      expect(
        dart.contains('static const $role = Color(0xFF$hex)'),
        isTrue,
        reason: '$role is #$hex in the Rust and something else in main.dart',
      );
    }
  });
}
