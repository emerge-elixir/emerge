/* Test executable only (Linux/glibc). Count Rust system allocator and linked
 * native-library calls on the calling thread; never preload this into the VM. */
#define _GNU_SOURCE
#include <errno.h>
#include <stdint.h>
#include <stdlib.h>
#include <unistd.h>
extern void *__libc_malloc(size_t), *__libc_calloc(size_t,size_t), *__libc_realloc(void*,size_t), *__libc_memalign(size_t,size_t);
extern void __libc_free(void*);
static _Thread_local int enabled, fail_next;
/* Only arm around a growth request that uses try_reserve; disarm on first miss. */
void emerge_fail_next_reserve(void) { fail_next=1; }
static int deny(void) { if(!fail_next)return 0; fail_next=0;errno=ENOMEM;return 1; }
static _Thread_local uint64_t allocs, reallocs, frees;
void emerge_heap_start(void){ allocs=reallocs=frees=0; enabled=1; }
void emerge_heap_stop(uint64_t *out){ enabled=0; out[0]=allocs;out[1]=reallocs;out[2]=frees; }
void *malloc(size_t n){allocs+=enabled;return deny()?NULL:__libc_malloc(n);}
void *calloc(size_t n,size_t s){allocs+=enabled;return deny()?NULL:__libc_calloc(n,s);}
void *realloc(void *p,size_t n){reallocs+=enabled;return deny()?NULL:__libc_realloc(p,n);}
void *reallocarray(void *p,size_t n,size_t s){if(s&&n>SIZE_MAX/s){errno=ENOMEM;return NULL;}return realloc(p,n*s);}
void free(void*p){frees+=enabled;__libc_free(p);}
void *memalign(size_t a,size_t n){allocs+=enabled;return deny()?NULL:__libc_memalign(a,n);}
void *aligned_alloc(size_t a,size_t n){return memalign(a,n);}
int posix_memalign(void **out,size_t a,size_t n){if(a<sizeof(void*)||(a&(a-1)))return EINVAL;void*p=memalign(a,n);if(!p)return ENOMEM;*out=p;return 0;}
void *valloc(size_t n){return memalign((size_t)sysconf(_SC_PAGESIZE),n);}
void *pvalloc(size_t n){size_t page=(size_t)sysconf(_SC_PAGESIZE);if(n>SIZE_MAX-page+1){errno=ENOMEM;return NULL;}return memalign(page,(n+page-1)&~(page-1));}
