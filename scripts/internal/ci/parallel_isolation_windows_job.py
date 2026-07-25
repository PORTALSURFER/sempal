"""Windows Job Object ownership for the parallel-isolation runner."""

from __future__ import annotations

import ctypes
import json
import os
import subprocess
import sys
from pathlib import Path


class WindowsJob:
    """Own a Windows process tree through a Job Object."""

    class BasicAccountingInformation(ctypes.Structure):
        _fields_ = [
            ("total_user_time", ctypes.c_longlong),
            ("total_kernel_time", ctypes.c_longlong),
            ("this_period_total_user_time", ctypes.c_longlong),
            ("this_period_total_kernel_time", ctypes.c_longlong),
            ("total_page_fault_count", ctypes.c_uint32),
            ("total_processes", ctypes.c_uint32),
            ("active_processes", ctypes.c_uint32),
            ("total_terminated_processes", ctypes.c_uint32),
        ]

    def __init__(self, process: subprocess.Popen[str]) -> None:
        if os.name != "nt":
            raise RuntimeError("Windows Job Objects are only available on Windows")
        self.kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
        self.kernel32.CreateJobObjectW.argtypes = [ctypes.c_void_p, ctypes.c_wchar_p]
        self.kernel32.CreateJobObjectW.restype = ctypes.c_void_p
        self.kernel32.AssignProcessToJobObject.argtypes = [
            ctypes.c_void_p,
            ctypes.c_void_p,
        ]
        self.kernel32.AssignProcessToJobObject.restype = ctypes.c_int
        self.kernel32.QueryInformationJobObject.argtypes = [
            ctypes.c_void_p,
            ctypes.c_int,
            ctypes.c_void_p,
            ctypes.c_uint32,
            ctypes.POINTER(ctypes.c_uint32),
        ]
        self.kernel32.QueryInformationJobObject.restype = ctypes.c_int
        self.kernel32.TerminateJobObject.argtypes = [ctypes.c_void_p, ctypes.c_uint32]
        self.kernel32.TerminateJobObject.restype = ctypes.c_int
        self.kernel32.CloseHandle.argtypes = [ctypes.c_void_p]
        self.kernel32.CloseHandle.restype = ctypes.c_int
        self.handle = self.kernel32.CreateJobObjectW(None, None)
        if not self.handle:
            raise ctypes.WinError(ctypes.get_last_error())
        process_handle = int(getattr(process, "_handle"))
        if not self.kernel32.AssignProcessToJobObject(self.handle, process_handle):
            error = ctypes.WinError(ctypes.get_last_error())
            self.close()
            raise error

    def active_processes(self) -> int:
        """Return the number of live processes still owned by the job."""
        information = self.BasicAccountingInformation()
        returned_length = ctypes.c_uint32()
        succeeded = self.kernel32.QueryInformationJobObject(
            self.handle,
            1,
            ctypes.byref(information),
            ctypes.sizeof(information),
            ctypes.byref(returned_length),
        )
        if not succeeded:
            raise ctypes.WinError(ctypes.get_last_error())
        return int(information.active_processes)

    def terminate(self) -> None:
        """Terminate every process still owned by the job."""
        if self.handle and not self.kernel32.TerminateJobObject(self.handle, 1):
            error_code = ctypes.get_last_error()
            if error_code:
                raise ctypes.WinError(error_code)

    def close(self) -> None:
        """Release the Job Object handle."""
        if self.handle:
            self.kernel32.CloseHandle(self.handle)
            self.handle = None


def bootstrap_command(command: list[str], runner_path: Path) -> list[str]:
    """Build a child command that waits until its Job Object is attached."""
    return [
        sys.executable,
        str(runner_path),
        "--windows-job-bootstrap",
        json.dumps(command),
    ]


def run_bootstrap(command_json: str) -> int:
    """Wait for ownership handoff, then run the actual test command inside the job."""
    if os.name != "nt":
        return 125
    if sys.stdin.read(1) != "1":
        return 125
    command = json.loads(command_json)
    if not isinstance(command, list) or not all(isinstance(item, str) for item in command):
        return 125
    return subprocess.run(command, check=False).returncode
