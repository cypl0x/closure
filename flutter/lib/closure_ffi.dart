// Dart bindings for `crates/closure-ffi`, over `include/closure.h`.
//
// Hand-written against the header rather than generated, for the same
// reason the header is hand-written: the three rules that make this
// boundary safe do not appear in any signature.
//
//   1. Every pointer the library returns is freed by a closure_*_free.
//      `malloc.free` on one of them would be freeing a Rust allocation
//      with a C allocator.
//   2. Null is always an acceptable argument.
//   3. Nothing panics across the boundary — the Rust side catches, so
//      a bug there arrives here as a null, not as a dead isolate.
//
// This file adds a fourth rule that is Dart's problem alone: a session
// that has been closed must refuse to be used, because Dart will
// happily hand a freed pointer to a widget rebuild.

import 'dart:ffi' as ffi;
import 'dart:io';

import 'package:ffi/ffi.dart';

typedef _AbiVersionC = ffi.Size Function();
typedef _AbiVersion = int Function();
typedef _OpenC = ffi.Pointer<ffi.Opaque> Function(ffi.Pointer<ffi.Char>);
typedef _Open = ffi.Pointer<ffi.Opaque> Function(ffi.Pointer<ffi.Char>);
typedef _CloseC = ffi.Void Function(ffi.Pointer<ffi.Opaque>);
typedef _Close = void Function(ffi.Pointer<ffi.Opaque>);
typedef _RowCountC = ffi.Size Function(ffi.Pointer<ffi.Opaque>);
typedef _RowCount = int Function(ffi.Pointer<ffi.Opaque>);
typedef _RowTitleC = ffi.Pointer<ffi.Char> Function(
    ffi.Pointer<ffi.Opaque>, ffi.Size);
typedef _RowTitle = ffi.Pointer<ffi.Char> Function(
    ffi.Pointer<ffi.Opaque>, int);
typedef _SelectC = ffi.Void Function(ffi.Pointer<ffi.Opaque>, ffi.Size);
typedef _Select = void Function(ffi.Pointer<ffi.Opaque>, int);
typedef _BodyC = ffi.Pointer<ffi.Char> Function(ffi.Pointer<ffi.Opaque>);
typedef _Body = ffi.Pointer<ffi.Char> Function(ffi.Pointer<ffi.Opaque>);
typedef _StringFreeC = ffi.Void Function(ffi.Pointer<ffi.Char>);
typedef _StringFree = void Function(ffi.Pointer<ffi.Char>);

/// The loaded library, and the version handshake.
class Closure {
  /// The `CLOSURE_ABI_VERSION` this file was written against. If the
  /// header's number moves and this one does not, the handshake fails
  /// loudly instead of the two sides quietly disagreeing about what a
  /// pointer means.
  static const int expectedAbiVersion = 1;

  static Closure? _instance;

  final ffi.DynamicLibrary _lib;
  final _AbiVersion abiVersionFn;
  final _Open open;
  final _Close closeSession;
  final _RowCount rowCount;
  final _RowTitle rowTitle;
  final _Select select;
  final _Body selectedBody;
  final _StringFree stringFree;

  Closure._(this._lib)
      : abiVersionFn =
            _lib.lookupFunction<_AbiVersionC, _AbiVersion>('closure_ffi_abi_version'),
        open = _lib.lookupFunction<_OpenC, _Open>('closure_open'),
        closeSession = _lib.lookupFunction<_CloseC, _Close>('closure_close'),
        rowCount = _lib.lookupFunction<_RowCountC, _RowCount>('closure_row_count'),
        rowTitle = _lib.lookupFunction<_RowTitleC, _RowTitle>('closure_row_title'),
        select = _lib.lookupFunction<_SelectC, _Select>('closure_select'),
        selectedBody = _lib.lookupFunction<_BodyC, _Body>('closure_selected_body'),
        stringFree =
            _lib.lookupFunction<_StringFreeC, _StringFree>('closure_string_free');

  /// The library's own ABI version.
  int get abiVersion => abiVersionFn();

  static Closure get instance => _instance ??= Closure._(_load());

  /// Where the `.so` is.
  ///
  /// A bundled app finds it beside the executable; `flutter test` and a
  /// developer run find it in the cargo target directory. `CLOSURE_FFI_LIB`
  /// overrides both, which is how `just flutter` points at whichever
  /// profile it just built.
  static ffi.DynamicLibrary _load() {
    const name = 'libclosure_ffi.so';
    final override = Platform.environment['CLOSURE_FFI_LIB'];
    final exeDir = File(Platform.resolvedExecutable).parent.path;
    final candidates = [
      if (override != null && override.isNotEmpty) override,
      '$exeDir/lib/$name',
      '$exeDir/$name',
      'target/release/$name',
      'target/debug/$name',
      '../target/release/$name',
      '../target/debug/$name',
    ];
    for (final c in candidates) {
      if (File(c).existsSync()) {
        return ffi.DynamicLibrary.open(c);
      }
    }
    throw StateError(
      'libclosure_ffi.so not found. Build it with `cargo build -p closure-ffi` '
      'or set CLOSURE_FFI_LIB. Looked in: ${candidates.join(", ")}',
    );
  }
}

/// One open vault.
///
/// Every method throws [StateError] after [close], rather than passing a
/// dangling pointer to a library that has every right to trust it.
class ClosureSession {
  ffi.Pointer<ffi.Opaque>? _handle;
  final Closure _c;

  ClosureSession._(this._c, this._handle);

  /// Open the vault at [path]. Null if it is not a vault we can read —
  /// the same answer the C side gives, not an exception, because "there
  /// is no vault there" is an ordinary thing for a file picker to hear.
  static ClosureSession? open(String path) {
    final c = Closure.instance;
    if (c.abiVersion != Closure.expectedAbiVersion) {
      throw StateError(
        'libclosure_ffi.so is ABI version ${c.abiVersion}, these bindings '
        'expect ${Closure.expectedAbiVersion}. Rebuild one of them.',
      );
    }
    final p = path.toNativeUtf8();
    try {
      final h = c.open(p.cast<ffi.Char>());
      return h == ffi.nullptr ? null : ClosureSession._(c, h);
    } finally {
      malloc.free(p);
    }
  }

  ffi.Pointer<ffi.Opaque> get _live {
    final h = _handle;
    if (h == null) {
      throw StateError('this session is closed');
    }
    return h;
  }

  /// How many outline rows the vault has.
  int get rowCount => _c.rowCount(_live);

  /// The title of row [index], or null if there is no such row.
  String? rowTitle(int index) => _take(_c.rowTitle(_live, index));

  /// Move the cursor. Out of range does nothing, as on the C side.
  void select(int index) => _c.select(_live, index);

  /// The selected headline's body as a reader should see it.
  String? get selectedBody => _take(_c.selectedBody(_live));

  /// Copy a returned string into Dart and hand the original back to the
  /// library that allocated it. Rule 1, in the one place it can be got
  /// wrong.
  String? _take(ffi.Pointer<ffi.Char> p) {
    if (p == ffi.nullptr) {
      return null;
    }
    final s = p.cast<Utf8>().toDartString();
    _c.stringFree(p);
    return s;
  }

  /// Close the vault. Closing twice is allowed and does nothing.
  void close() {
    final h = _handle;
    if (h != null) {
      _handle = null;
      _c.closeSession(h);
    }
  }
}
