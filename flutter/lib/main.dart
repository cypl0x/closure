// The Flutter shell: an outline and the selected body, over the C ABI.
//
// Deliberately small. `docs/spec.md` says a shell offers a subset of
// what the kernel understands and must not invent behaviour of its own,
// and this one offers Browse — the vault's headlines, and what a
// headline says. Everything it shows comes back from `closure-shell-core`
// through `closure-ffi`; there is no org parsing on this side of the
// boundary and there must never be, because that is how two owners of
// one fact get created.
//
// The palette is `doom_vibrant()` from `closure-shell-core`, copied
// rather than read: the ABI does not carry a theme yet, and a shell
// that guessed its own colours would drift from every other one. The
// test below is what keeps the copy honest.

import 'package:flutter/material.dart';

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

void main(List<String> args) {
  final path = args.isNotEmpty
      ? args.first
      : const String.fromEnvironment('CLOSURE_VAULT', defaultValue: '');
  runApp(ClosureApp(vaultPath: path));
}

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
          Expanded(child: _panes(s, count)),
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
              child: ListView.builder(
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
        'Browse only — no editing, capture, agenda or keybindings. '
        'Needs a GL-capable display; see "The Flutter shell" in docs/spec.md.',
        style: TextStyle(color: Palette.muted, fontSize: 11),
      ),
    );
  }
}
