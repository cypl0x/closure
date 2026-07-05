# Q4 — eglot smoke test for `closure lsp` (user-driven)

`closure lsp VAULT` speaks Content-Length-framed JSON-RPC on stdio and
advertises: documentSymbol, hover, completion (trigger `:`), pull
diagnostics, references, rename (server-authoritative, I8).

## Emacs setup (Doom)

Add to `config.el` (adjust the vault path):

```elisp
(after! eglot
  ;; closure vault notes: attach eglot to org buffers under the vault.
  (add-to-list 'eglot-server-programs
               `((org-mode :language-id "org")
                 . ("closure" "lsp" ,(expand-file-name "~/vault")))))

;; Optional: auto-start in vault files only.
(defun +closure-eglot-maybe ()
  (when (and buffer-file-name
             (string-prefix-p (expand-file-name "~/vault") buffer-file-name))
    (eglot-ensure)))
(add-hook 'org-mode-hook #'+closure-eglot-maybe)
```

Release binary first (memory-capped):

```sh
systemd-run --user --scope -p MemoryHigh=6G -p MemoryMax=8G -- \
  nix develop -c cargo build --release -p closure-cli -j 4
# put target/release/closure on PATH, or write the absolute path
# into eglot-server-programs above.
```

## Smoke checklist

Open a vault `.org` file, `M-x eglot` (or rely on the hook), then:

1. **Attach**: modeline shows `eglot:closure`; `M-x eglot-events-buffer`
   shows the `initialize` round trip with the capability set above.
2. **Symbols**: `M-x imenu` (or consult-imenu) lists the headlines.
3. **Hover**: cursor on a headline, `M-x eldoc` / idle — headline
   preview appears.
4. **Completion**: type `:` in a properties drawer / body — company or
   corfu offers org keyword candidates.
5. **Diagnostics**: `M-x flymake-show-buffer-diagnostics` — parse
   warnings (if any) listed; a clean file shows none.
6. **References**: cursor on a `[[link]]` target, `M-x xref-find-references`
   — backlink locations listed.
7. **Rename**: cursor on a headline, `M-x eglot-rename` NEWNAME — the
   file on disk changes (server-authoritative write; buffer reverts).

## Report back

Any step failing: paste the relevant slice of `eglot-events-buffer`
(the request/response pair) — each failure becomes a leaf in the next
orchestrated queue.
