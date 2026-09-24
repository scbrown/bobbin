"""Tiny real ELF fixtures for the runtime loader controls (no GPU required)."""
import subprocess

import pytest

from runner.runtime_lock import digest_file


@pytest.fixture
def runtime_config(tmp_path, monkeypatch):
    for name in ('LD_PRELOAD', 'LD_AUDIT', 'LD_LIBRARY_PATH'):
        monkeypatch.delenv(name, raising=False)
    root = tmp_path / 'runtime'
    root.mkdir()
    source = root / 'fixture.c'
    libraries = {}
    for name, body, dependencies in [
        ('libcudart.so.12', 'int cuda_fixture(void) { return 7; }', []),
        ('libonnxruntime.so.1', ORT_FIXTURE, ['-ldl']),
        ('libonnxruntime_providers_shared.so', 'int host_ready = 0; void Provider_SetHost(void) { host_ready = 1; }', []),
        ('libonnxruntime_providers_cuda.so',
         ('extern int cuda_fixture(void); extern int host_ready; '
         '__attribute__((constructor)) void init(void) { if (!host_ready) __builtin_trap(); } '
          'int provider_fixture(void) { return cuda_fixture(); }'),
         ['-L' + str(root), '-l:libcudart.so.12']),
    ]:
        source.write_text(body)
        path = root / name
        subprocess.run(['cc', '-shared', '-fPIC', '-Wl,-soname,' + name,
                        str(source), '-o', str(path), *dependencies], check=True, capture_output=True)
        libraries[str(path)] = digest_file(path)
    return {'library_lock_version': 1, 'ort_api': 23, 'libraries': libraries,
            'environment': {'ORT_DYLIB_PATH': str(root / 'libonnxruntime.so.1'),
                            'LD_LIBRARY_PATH': str(root)}}


# The provider constructor needs the host handshake, like real ORT. A plain
# dlopen probe crashes rather than receiving this fixture's green control.
ORT_FIXTURE = r"""
#define _GNU_SOURCE
#include <dlfcn.h>
#include <stdio.h>
#include <string.h>
#include <libgen.h>
static void *create_options(void **out) { *out = (void *)1; return 0; }
static const char *error_message(void *status) { return status; }
static void *api[11] = {[2] = error_message, [10] = create_options};
static void *get_api(unsigned version) { return version == 23 ? api : 0; }
static void *base[2] = {get_api, 0};
void *OrtGetApiBase(void) { return base; }
void *OrtSessionOptionsAppendExecutionProvider_CUDA(void *options, int device) {
    Dl_info info;
    dladdr(OrtGetApiBase, &info);
    char name[4096], path[4096];
    snprintf(name, sizeof(name), "%s", info.dli_fname);
    char *root = dirname(name);
    snprintf(path, sizeof(path), "%s/libonnxruntime_providers_shared.so", root);
    void *shared = dlopen(path, RTLD_NOW | RTLD_GLOBAL);
    if (!shared) return (void *)dlerror();
    void (*set_host)(void) = dlsym(shared, "Provider_SetHost");
    if (!set_host) return (void *)dlerror();
    set_host();
    snprintf(path, sizeof(path), "%s/libonnxruntime_providers_cuda.so", root);
    if (!dlopen(path, RTLD_NOW | RTLD_LOCAL)) return (void *)dlerror();
    return 0;
}
"""
