// SPDX-License-Identifier: Apache-2.0
// MSVC OpenMP entrypoints used by the locked CPU PyTorch wheel. All team state
// belongs to LLVM libomp, including regions entered through these wrappers.
#include "kmp.h"
#include <array>
#include <cstdarg>
#include <cstdlib>
#include <utility>

extern "C" void __kmpc_for_static_init_8u(
    ident_t *, kmp_int32, kmp_int32, kmp_int32 *, kmp_uint64 *, kmp_uint64 *,
    kmp_int64 *, kmp_int64, kmp_int64);

namespace {
ident_t location = {0, 2, 0, 0, ";ariadshift;vcomp;0;0;;"};
using Callback = void (*)();
constexpr int max_arguments = 256;
struct Region {
  Callback callback;
  int count;
  std::array<void *, max_arguments> arguments;
};

template <size_t... I>
void invoke(Region &region, std::index_sequence<I...>) {
  using Function = void (*)(decltype((void)I, (void *)nullptr)...);
  reinterpret_cast<Function>(region.callback)(region.arguments[I]...);
}

template <size_t... N>
constexpr auto dispatch_table(std::index_sequence<N...>) {
  return std::array<void (*)(Region &), sizeof...(N)>{
      +[](Region &region) { invoke(region, std::make_index_sequence<N>{}); }...};
}

void microtask(kmp_int32 *, kmp_int32 *, Region *region) {
  static constexpr auto dispatch =
      dispatch_table(std::make_index_sequence<max_arguments + 1>{});
  dispatch[region->count](*region);
}
} // namespace

extern "C" {
__declspec(dllexport) void _vcomp_fork(int parallel, int count, Callback callback, ...) {
  if (count < 0 || count > max_arguments || !callback)
    std::abort();
  Region region{callback, count, {}};
  va_list arguments;
  va_start(arguments, callback);
  for (int i = 0; i < count; ++i)
    region.arguments[i] = va_arg(arguments, void *);
  va_end(arguments);
  if (parallel) {
    __kmpc_fork_call(&location, 1, reinterpret_cast<kmpc_micro>(microtask), &region);
  } else {
    auto tid = __kmpc_global_thread_num(&location);
    __kmpc_serialized_parallel(&location, tid);
    microtask(nullptr, nullptr, &region);
    __kmpc_end_serialized_parallel(&location, tid);
  }
}

__declspec(dllexport) void _vcomp_set_num_threads(int count) {
  __kmpc_push_num_threads(&location, __kmpc_global_thread_num(&location), count);
}

__declspec(dllexport) void _vcomp_barrier() {
  __kmpc_barrier(&location, __kmpc_global_thread_num(&location));
}

__declspec(dllexport) void _vcomp_for_static_simple_init_i8(
    kmp_uint64 first, kmp_uint64 last, kmp_int64 step, int increment,
    kmp_uint64 *begin, kmp_uint64 *end) {
  kmp_int32 last_iteration;
  kmp_int64 stride;
  if (step <= 0)
    std::abort();
  // Schedule normalized iteration numbers so signed loops crossing zero and
  // unsigned loops above INT64_MAX share the same LLVM scheduling operation.
  auto distance = increment ? last - first : first - last;
  kmp_uint64 lower = 0, upper = distance / static_cast<kmp_uint64>(step);
  auto final_iteration = upper;
  __kmpc_for_static_init_8u(&location, __kmpc_global_thread_num(&location),
                          kmp_sch_static, &last_iteration, &lower, &upper,
                          &stride, 1, 1);
  if (lower > upper || lower > final_iteration) {
    *begin = increment ? 1 : 0;
    *end = increment ? 0 : 1;
    return;
  }
  if (upper > final_iteration)
    upper = final_iteration;
  *begin = increment ? first + lower * step : first - lower * step;
  *end = increment ? first + upper * step : first - upper * step;
}

__declspec(dllexport) void _vcomp_for_static_end() {
  __kmpc_for_static_fini(&location, __kmpc_global_thread_num(&location));
}
}
