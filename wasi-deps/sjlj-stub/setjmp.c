/* Stub setjmp/longjmp for WASI builds without exception handling.
 *
 * setjmp() always returns 0.
 * longjmp() aborts — the WASM runtime catches the resulting trap.
 */

#define _GNU_SOURCE
#include "setjmp.h"
#include <stdlib.h>
#include <stdio.h>

int setjmp(jmp_buf buf) {
    (void)buf;
    return 0;
}

void longjmp(jmp_buf buf, int val) {
    (void)buf;
    (void)val;
    fprintf(stderr, "fatal: longjmp called (TeX engine abort)\n");
    abort();
}

int _setjmp(jmp_buf buf) {
    (void)buf;
    return 0;
}

void _longjmp(jmp_buf buf, int val) {
    (void)buf;
    (void)val;
    fprintf(stderr, "fatal: _longjmp called (TeX engine abort)\n");
    abort();
}

int sigsetjmp(sigjmp_buf buf, int savemask) {
    (void)buf;
    (void)savemask;
    return 0;
}

void siglongjmp(sigjmp_buf buf, int val) {
    (void)buf;
    (void)val;
    fprintf(stderr, "fatal: siglongjmp called (TeX engine abort)\n");
    abort();
}
