// SPDX-License-Identifier: Apache-2.0
// Verify the replacement against actual MSVC-generated OpenMP calls.
#include <omp.h>
#include <array>
#include <cstdio>

int main() {
  omp_set_dynamic(0);
  omp_set_num_threads(3);
  std::array<int, 37> visits{};
#pragma omp parallel for schedule(static)
  for (long long index = -18; index <= 18; index += 3)
    visits[static_cast<size_t>(index + 18)] += 1;
  for (size_t index = 0; index < visits.size(); ++index)
    if (visits[index] != (index % 3 == 0 ? 1 : 0))
      return 1;

  visits.fill(0);
#pragma omp parallel for schedule(static)
  for (long long index = 18; index >= -18; index -= 3)
    visits[static_cast<size_t>(index + 18)] += 1;
  for (size_t index = 0; index < visits.size(); ++index)
    if (visits[index] != (index % 3 == 0 ? 1 : 0))
      return 2;

  std::array<int, 3> teams{};
#pragma omp parallel num_threads(3)
  {
    teams[omp_get_thread_num()] = omp_get_num_threads();
#pragma omp barrier
  }
  for (auto team : teams)
    if (team != 3)
      return 3;

  int serialized = 0;
#pragma omp parallel if(0) num_threads(4)
  serialized += omp_get_num_threads();
  if (serialized != 1)
    return 4;
  std::puts("MSVC-generated signed static loops, team queries, barriers and serialization passed");
  return 0;
}
