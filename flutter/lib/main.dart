// The Flutter shell: an outline and the selected body, over the C ABI.
//
// Deliberately small. `docs/spec.md` says a shell offers a subset of
// what the kernel understands and must not invent behaviour of its own,
// and this one offers CORE and exactly CORE: the vault's headlines,
// what a headline says, a search that filters them, and a bar that
// files a TODO. Everything it shows comes back from `closure-shell-core`
// through `closure-ffi`; there is no org parsing on this side of the
// boundary and there must never be, because that is how two owners of
// one fact get created.
//
// The palette is `doom_vibrant()` from `closure-shell-core`, copied
// rather than read: the ABI does not carry a theme yet, and a shell
// that guessed its own colours would drift from every other one. The
// test below is what keeps the copy honest.

import 'dart:io';

import 'package:flutter/material.dart';
import 'package:path_provider/path_provider.dart';

import 'closure_ffi.dart';

/// `Theme::doom_vibrant()`, as hex, in the order the Rust declares them.
class Palette {
  static const fg = Color(0xFFBBC2CF);
  static const bg = Color(0xFF242730);
  static const accent = Color(0xFF51AFEF);
  static const muted = Color(0xFF62686E);
  static const selection = Color(0xFF3D4451);
  static const heading2 = Color(0xFFC57BDB);
}

void main(List<String> args) async {
  WidgetsFlutterBinding.ensureInitialized();
  runApp(ClosureApp(vaultPath: await resolveVault(args)));
}

/// Where this shell's vault is.
///
/// On a desktop the vault is an argument, because a desktop shell opens
/// the notes you already have and there is a shell around it to name
/// them. Android has no argv and no shell: an app is launched by
/// tapping it, so a vault it cannot be told about is a vault it does
/// not have — which is what an APK of this would have shown, an empty
/// window saying "No vault".
///
/// So on Android it is the app's own documents directory, seeded once
/// with a real org file. That is a different vault from the one on the
/// desktop and deliberately so: the phone has no access to the
/// desktop's files, and inventing a path into shared storage would ask
/// for a permission this shell has no way to justify yet.
Future<String> resolveVault(List<String> args) async {
  if (args.isNotEmpty) {
    return args.first;
  }
  const compiled = String.fromEnvironment('CLOSURE_VAULT', defaultValue: '');
  if (compiled.isNotEmpty) {
    return compiled;
  }
  if (!Platform.isAndroid && !Platform.isIOS) {
    return '';
  }
  final docs = await getApplicationDocumentsDirectory();
  final vault = Directory('${docs.path}/vault');
  if (!vault.existsSync()) {
    vault.createSync(recursive: true);
  }
  final notes = File('${vault.path}/notes.org');
  if (!notes.existsSync()) {
    notes.writeAsStringSync(SEED_NOTES);
  }
  return vault.path;
}

/// What a brand-new vault contains.
///
/// Not an empty file: an empty outline and a broken vault look
/// identical, and the first thing this app shows a new user should
/// demonstrate that it works rather than leave them wondering.
// ignore: constant_identifier_names
const SEED_NOTES = '''
* Welcome to closure
:PROPERTIES:
:ID: 01SEEDNOTE0000000001
:END:
This vault lives in the app's own documents directory.
It is plain org text — the same format every other closure shell reads.

* What this shell can do
:PROPERTIES:
:ID: 01SEEDNOTE0000000002
:END:
Browse the outline, search it, and capture a TODO.
No editing, no agenda, no keybindings — see the notice at the bottom.

* Capture something
:PROPERTIES:
:ID: 01SEEDNOTE0000000003
:END:
Type into the bar at the bottom and press Enter.
It is filed as a TODO by the same kernel every other shell uses.
''';

class ClosureApp extends StatelessWidget {
  final String vaultPath;

  const ClosureApp({super.key, required this.vaultPath});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'closure',
      debugShowCheckedModeBanner: false,
      theme: ThemeData(
        brightness: Brightness.dark,
        scaffoldBackgroundColor: Palette.bg,
        colorScheme: const ColorScheme.dark(
          surface: Palette.bg,
          primary: Palette.accent,
          onSurface: Palette.fg,
        ),
      ),
      home: VaultView(vaultPath: vaultPath),
    );
  }
}

class VaultView extends StatefulWidget {
  final String vaultPath;

  const VaultView({super.key, required this.vaultPath});

  @override
  State<VaultView> createState() => _VaultViewState();
}

class _VaultViewState extends State<VaultView> {
  ClosureSession? _session;
  String? _error;
  int _selected = 0;
  final _captureController = TextEditingController();

  @override
  void initState() {
    super.initState();
    if (widget.vaultPath.isEmpty) {
      _error = 'No vault. Pass one: closure_shell /path/to/vault';
      return;
    }
    final s = ClosureSession.open(widget.vaultPath);
    if (s == null) {
      _error = 'Not a vault this can read: ${widget.vaultPath}';
      return;
    }
    _session = s;
    if (s.rowCount > 0) {
      s.select(0);
    }
  }

  @override
  void dispose() {
    _captureController.dispose();
    _session?.close();
    _session = null;
    super.dispose();
  }

  void _select(int i) {
    setState(() {
      _selected = i;
      _session?.select(i);
    });
  }

  /// Filter as the user types. The kernel decides what matches; this
  /// hands it the text and repaints.
  void _search(String needle) {
    setState(() {
      _session?.search(needle);
      // The old selection indexed the old list. Anything else would
      // show one row highlighted and a different row's body.
      _selected = 0;
      if ((_session?.rowCount ?? 0) > 0) {
        _session?.select(0);
      }
    });
  }

  void _capture(String title) {
    final s = _session;
    if (s == null) {
      return;
    }
    // Refusal is the kernel's call, not a rule repeated here — a blank
    // title comes straight back as false.
    final filed = s.capture(title);
    setState(() {
      if (filed) {
        _captureController.clear();
      }
    });
  }

  @override
  Widget build(BuildContext context) {
    final err = _error;
    if (err != null) {
      return Scaffold(
        body: Center(
          child: Text(err, style: const TextStyle(color: Palette.muted)),
        ),
      );
    }
    final s = _session!;
    final count = s.rowCount;
    return Scaffold(
      body: Column(
        children: [
          _Bar(
            fieldKey: const Key('search'),
            hint: 'Search headlines',
            onChanged: _search,
          ),
          Expanded(child: _panes(s, count)),
          _Bar(
            fieldKey: const Key('capture'),
            hint: 'Capture a TODO, Enter to file it',
            controller: _captureController,
            onSubmitted: _capture,
          ),
          const _Notice(),
        ],
      ),
    );
  }

  Widget _panes(ClosureSession s, int count) {
    return Row(
        children: [
          SizedBox(
            width: 320,
            child: Container(
              color: Palette.bg,
              child: count == 0
                  ? const Padding(
                      padding: EdgeInsets.all(12),
                      child: Text(
                        'No matches.',
                        style: TextStyle(color: Palette.muted, fontSize: 13),
                      ),
                    )
                  : ListView.builder(
                itemCount: count,
                itemBuilder: (context, i) {
                  final selected = i == _selected;
                  return InkWell(
                    onTap: () => _select(i),
                    child: Container(
                      color: selected ? Palette.selection : null,
                      padding: const EdgeInsets.symmetric(
                          horizontal: 12, vertical: 6),
                      width: double.infinity,
                      child: Text(
                        s.rowTitle(i) ?? '',
                        style: TextStyle(
                          color: selected ? Palette.accent : Palette.fg,
                          fontSize: 14,
                        ),
                      ),
                    ),
                  );
                },
                    ),
            ),
          ),
          const VerticalDivider(width: 1, color: Palette.selection),
          Expanded(
            child: Container(
              color: Palette.bg,
              padding: const EdgeInsets.all(16),
              alignment: Alignment.topLeft,
              child: SingleChildScrollView(
                child: Text(
                  s.selectedBody ?? '',
                  style: const TextStyle(
                    color: Palette.fg,
                    fontSize: 14,
                    height: 1.5,
                  ),
                ),
              ),
            ),
          ),
        ],
      );
  }
}

/// A one-line text field with a label, used for search and for capture.
class _Bar extends StatelessWidget {
  final Key fieldKey;
  final String hint;
  final TextEditingController? controller;
  final ValueChanged<String>? onChanged;
  final ValueChanged<String>? onSubmitted;

  const _Bar({
    required this.fieldKey,
    required this.hint,
    this.controller,
    this.onChanged,
    this.onSubmitted,
  });

  @override
  Widget build(BuildContext context) {
    return Container(
      color: Palette.bg,
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
      child: TextField(
        key: fieldKey,
        controller: controller,
        onChanged: onChanged,
        onSubmitted: onSubmitted,
        style: const TextStyle(color: Palette.fg, fontSize: 13),
        cursorColor: Palette.accent,
        decoration: InputDecoration(
          isDense: true,
          hintText: hint,
          hintStyle: const TextStyle(color: Palette.muted, fontSize: 13),
          enabledBorder: const UnderlineInputBorder(
            borderSide: BorderSide(color: Palette.selection),
          ),
          focusedBorder: const UnderlineInputBorder(
            borderSide: BorderSide(color: Palette.accent),
          ),
        ),
      ),
    );
  }
}

/// What this shell does not do, said where the user is.
///
/// The gpui window carries the equivalent about the software rasteriser
/// and held keys. The reasoning is the same: a shell offers a subset of
/// what the kernel understands, and one that hides which subset teaches
/// a wrong model of the system — the user concludes closure cannot edit,
/// rather than that this window cannot.
class _Notice extends StatelessWidget {
  const _Notice();

  @override
  Widget build(BuildContext context) {
    return Container(
      width: double.infinity,
      color: Palette.selection,
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
      child: const Text(
        'Browse, search and capture only — no editing, agenda or keybindings. '
        'Needs a GL-capable display; see "The Flutter shell" in docs/spec.md.',
        style: TextStyle(color: Palette.muted, fontSize: 11),
      ),
    );
  }
}
