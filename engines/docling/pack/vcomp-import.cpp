// SPDX-License-Identifier: Apache-2.0
// Import-library-only markers, following LLVM's former kmp_import.c mechanism.
// MSVC forces a reference to the Microsoft marker for objects built with /openmp.
// Keep these definitions out of the DLL; all OpenMP operations run in LLVM.
extern "C" {
int _You_must_link_with_exactly_one_OpenMP_library = 1;
int _You_must_link_with_Microsoft_OpenMP_library = 1;
}
