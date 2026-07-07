/* Stub setjmp/longjmp for WASI builds without exception handling.
 *
 * setjmp() always returns 0 (the "normal" path is always taken).
 *
 * A real setjmp/longjmp cannot be implemented without WebAssembly exception
 * handling, which we deliberately avoid so the module runs on any WASM runtime.
 * So a longjmp (the engine's fatal-error path, _tt_abort) cannot resume at the
 * matching setjmp; the run must end here.
 *
 * Rather than abort() — which lowers to an untyped `unreachable` trap that the
 * host cannot tell apart from a genuine engine bug or an out-of-memory kill —
 * we end the instance with a WASI proc_exit carrying a reserved status code.
 * The host (wazero) surfaces that as a clean exit code, giving it a typed,
 * machine-readable signal that "the TeX engine aborted this run" without having
 * to scrape stderr. This keeps the module portable (no WASM EH instructions)
 * and costs nothing on the success path.
 */

#define _GNU_SOURCE
#include "setjmp.h"
#include <stdlib.h>
#include <stdio.h>

/* Reserved WASI exit status for a controlled TeX engine abort via longjmp.
 * This is a contract with the embedding host (see tecgonic's texAbortExitCode).
 * It must not collide with a status the engine would proc_exit with for any
 * other reason; the reactor never calls exit() on a normal path, so any
 * distinctive non-zero value is safe. */
#define TT_ABORT_EXIT_CODE 42

static _Noreturn void tt_abort_exit(const char *what) {
    fprintf(stderr, "fatal: %s called (TeX engine abort)\n", what);
    fflush(NULL); /* ensure the diagnostic tail reaches the host before exit */
    _Exit(TT_ABORT_EXIT_CODE);
}

int setjmp(jmp_buf buf) {
    (void)buf;
    return 0;
}

void longjmp(jmp_buf buf, int val) {
    (void)buf;
    (void)val;
    tt_abort_exit("longjmp");
}

int _setjmp(jmp_buf buf) {
    (void)buf;
    return 0;
}

void _longjmp(jmp_buf buf, int val) {
    (void)buf;
    (void)val;
    tt_abort_exit("_longjmp");
}

int sigsetjmp(sigjmp_buf buf, int savemask) {
    (void)buf;
    (void)savemask;
    return 0;
}

void siglongjmp(sigjmp_buf buf, int val) {
    (void)buf;
    (void)val;
    tt_abort_exit("siglongjmp");
}
