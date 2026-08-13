/* closure — C ABI over closure-shell-core.
 *
 * Hand-written rather than generated, because this is the document a
 * binding author reads, and a generator emits signatures without the
 * three rules that make them safe to call:
 *
 *   1. Every pointer this library returns is freed by a closure_*_free
 *      from this library. Do not call free().
 *   2. NULL is always an acceptable argument and never crashes.
 *   3. Nothing panics or throws across this boundary.
 *
 * Check closure_ffi_abi_version() before anything else: a mismatch
 * between the .so and the bindings is the one failure that produces
 * silent corruption rather than an error.
 */
#ifndef CLOSURE_H
#define CLOSURE_H

#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Bumped whenever a signature below changes meaning. */
#define CLOSURE_ABI_VERSION 1

typedef struct ClosureSession ClosureSession;

/* The ABI version this library was built with. */
size_t closure_ffi_abi_version(void);

/* Open a vault. NULL if the path is NULL, not UTF-8, or not readable. */
ClosureSession *closure_open(const char *path);

/* Close a session. NULL is allowed. */
void closure_close(ClosureSession *handle);

/* Number of outline rows. 0 for NULL. */
size_t closure_row_count(ClosureSession *handle);

/* Title of row `index`, or NULL. Free with closure_string_free. */
char *closure_row_title(ClosureSession *handle, size_t index);

/* Move the cursor. Out-of-range and NULL are no-ops. */
void closure_select(ClosureSession *handle, size_t index);

/* The selected body as a reader should see it, or NULL.
 * Free with closure_string_free. */
char *closure_selected_body(ClosureSession *handle);

/* Free a string this library returned. NULL is allowed. */
void closure_string_free(char *s);

#ifdef __cplusplus
}
#endif

#endif /* CLOSURE_H */
