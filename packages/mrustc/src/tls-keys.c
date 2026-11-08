/*
 * More pthread keys than musl's 128, for the rustc mrustc compiles
 * (packages/mrustc/README.md). mrustc emits no native thread-locals, so
 * each of that rustc's thread_local!s is a pthread key, and rustc 1.90.0
 * has more than musl allows. Loaded with LD_PRELOAD for that rustc
 * alone: each thread's values are a table of its own, kept under one
 * real musl key, whose destructor runs the keys' destructors as
 * pthread_exit would, up to PTHREAD_DESTRUCTOR_ITERATIONS rounds.
 */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <limits.h>
#include <pthread.h>
#include <stdatomic.h>
#include <stdlib.h>

#define KEYS 4096

struct table {
    void *values[KEYS];
};

static void (*destructors[KEYS])(void *);
static atomic_uint next_key;
static pthread_key_t real_key;
static pthread_once_t once = PTHREAD_ONCE_INIT;
static int (*real_create)(pthread_key_t *, void (*)(void *));
static void *(*real_get)(pthread_key_t);
static int (*real_set)(pthread_key_t, const void *);

static void run_destructors(void *pointer)
{
    struct table *table = pointer;
    /* The table is this thread's again while its keys' destructors
     * run, which may read or set other keys. */
    real_set(real_key, table);
    for (int round = 0; round < PTHREAD_DESTRUCTOR_ITERATIONS; round++) {
        int ran = 0;
        unsigned keys = atomic_load(&next_key);
        for (unsigned key = 0; key < keys && key < KEYS; key++) {
            void *value = table->values[key];
            if (value && destructors[key]) {
                table->values[key] = 0;
                destructors[key](value);
                ran = 1;
            }
        }
        if (!ran)
            break;
    }
    real_set(real_key, 0);
    free(table);
}

static void init(void)
{
    real_create = (int (*)(pthread_key_t *, void (*)(void *)))dlsym(RTLD_NEXT, "pthread_key_create");
    real_get = (void *(*)(pthread_key_t))dlsym(RTLD_NEXT, "pthread_getspecific");
    real_set = (int (*)(pthread_key_t, const void *))dlsym(RTLD_NEXT, "pthread_setspecific");
    if (!real_create || !real_get || !real_set || real_create(&real_key, run_destructors) != 0)
        abort();
}

int pthread_key_create(pthread_key_t *key, void (*destructor)(void *))
{
    pthread_once(&once, init);
    unsigned made = atomic_fetch_add(&next_key, 1);
    if (made >= KEYS)
        return EAGAIN;
    destructors[made] = destructor;
    *key = made;
    return 0;
}

int pthread_key_delete(pthread_key_t key)
{
    if (key >= KEYS)
        return EINVAL;
    destructors[key] = 0;
    return 0;
}

void *pthread_getspecific(pthread_key_t key)
{
    pthread_once(&once, init);
    struct table *table = real_get(real_key);
    return table && key < KEYS ? table->values[key] : 0;
}

int pthread_setspecific(pthread_key_t key, const void *value)
{
    pthread_once(&once, init);
    if (key >= KEYS)
        return EINVAL;
    struct table *table = real_get(real_key);
    if (!table) {
        table = calloc(1, sizeof *table);
        if (!table)
            return ENOMEM;
        real_set(real_key, table);
    }
    table->values[key] = (void *)value;
    return 0;
}
