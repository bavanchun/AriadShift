"""Exercise real cv2, PyTorch parallel math and the Windows MSVC OpenMP ABI."""

from __future__ import annotations

import ctypes
import sys
from pathlib import Path

import cv2
import torch


def check(condition, message):
    if not condition:
        raise SystemExit(message)


def torch_math():
    torch.manual_seed(42)
    torch.set_num_threads(1)
    matrix = torch.randn(257, 193)
    other = torch.randn(193, 129)
    vector = torch.arange(100003, dtype=torch.float64)
    expected = (matrix @ other, vector.sum(), vector.sin() + vector.cos())
    for threads in (2, 3, 4):
        torch.set_num_threads(threads)
        for _ in range(3):
            actual = (matrix @ other, vector.sum(), vector.sin() + vector.cos())
            for left, right in zip(expected, actual, strict=True):
                torch.testing.assert_close(left, right)
    torch.set_num_threads(3)
    print(
        "PyTorch matrix multiplication, reduction and elementwise math agree across thread counts"
    )


def vcomp_abi():
    dll = ctypes.CDLL(str(Path(torch.__file__).parent / "lib/libiomp5md.dll"))
    integer = ctypes.c_int
    unsigned = ctypes.c_uint64
    pointer = ctypes.c_void_p
    dll._vcomp_set_num_threads.argtypes = [integer]
    dll._vcomp_for_static_simple_init_i8.argtypes = [
        unsigned,
        unsigned,
        ctypes.c_int64,
        integer,
        ctypes.POINTER(unsigned),
        ctypes.POINTER(unsigned),
    ]
    dll._vcomp_barrier.argtypes = []
    dll._vcomp_for_static_end.argtypes = []
    dll.omp_get_thread_num.restype = integer
    dll.omp_get_num_threads.restype = integer

    for count in (0, 1, 4, 8, 32, 256):
        observed = []
        signature = ctypes.CFUNCTYPE(None, *([pointer] * count))

        @signature
        def callback(*arguments, observed=observed):
            observed.append((dll.omp_get_thread_num(), dll.omp_get_num_threads(), arguments))
            dll._vcomp_barrier()

        for parallel, team in ((1, 3), (0, 1)):
            observed.clear()
            dll._vcomp_set_num_threads(3)
            dll._vcomp_fork(
                integer(count), integer(parallel), callback, *[pointer(i + 1) for i in range(count)]
            )
            check(len(observed) == team, "MSVC fork produced the wrong team size")
            check({row[0] for row in observed} == set(range(team)), "MSVC thread IDs differ")
            check(
                all(row[1] == team and row[2] == tuple(range(1, count + 1)) for row in observed),
                "MSVC capture arguments or team state differ",
            )

    for first, last, step in (
        (0, 31, 1),
        (2, 35, 3),
        (35, 2, -3),
        (-12, 12, 2),
        (12, -12, -2),
        (0, 0, 1),
        (2**63 + 4, 2**63 + 40, 4),
    ):
        observed = []
        signature = ctypes.CFUNCTYPE(None)

        @signature
        def loop(first=first, last=last, step=step, observed=observed):
            begin, end = unsigned(), unsigned()
            dll._vcomp_for_static_simple_init_i8(
                first, last, abs(step), int(step > 0), ctypes.byref(begin), ctypes.byref(end)
            )
            if first < 0 or last < 0:
                lower, upper = ctypes.c_int64(begin.value).value, ctypes.c_int64(end.value).value
            else:
                lower, upper = begin.value, end.value
            observed.extend(range(lower, upper + (1 if step > 0 else -1), step))
            dll._vcomp_for_static_end()
            dll._vcomp_barrier()

        dll._vcomp_set_num_threads(3)
        dll._vcomp_fork(integer(0), integer(1), loop)
        expected = list(range(first, last + (1 if step > 0 else -1), step))
        check(
            sorted(observed) == sorted(expected), "MSVC static scheduling lost or duplicated work"
        )
    print("MSVC fork, captured arguments, serialized regions, static loops and barriers passed")


if __name__ == "__main__":
    check(not cv2.ipp.useIPP(), "OpenCV enables IPP")
    torch_math()
    if sys.platform == "win32":
        vcomp_abi()
