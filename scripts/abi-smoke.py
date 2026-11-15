"""Exercise the built plugin ABI in an isolated temporary directory, no credentials."""
import ctypes
import json
import pathlib
import tempfile
import sys

library = ctypes.CDLL(sys.argv[1])
library.otools_plugin_invoke.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.POINTER(ctypes.c_size_t)]
library.otools_plugin_invoke.restype = ctypes.c_void_p
library.otools_plugin_free.argtypes = [ctypes.c_void_p, ctypes.c_size_t]

def invoke(method, payload, data):
    request = json.dumps({"method": method, "payload": payload, "pluginDataDir": str(data), "pluginDir": str(data)}).encode()
    buf = ctypes.create_string_buffer(request)
    size = ctypes.c_size_t()
    ptr = library.otools_plugin_invoke(buf, len(request), ctypes.byref(size))
    assert ptr and size.value
    try:
        value = json.loads(ctypes.string_at(ptr, size.value))
        if not value.get("ok"):
            raise AssertionError(value.get("error", "native error"))
        return value["data"]
    finally:
        library.otools_plugin_free(ptr, size.value)

with tempfile.TemporaryDirectory(prefix="releasehub-abi-") as folder:
    data = pathlib.Path(folder).resolve()
    assert invoke("__otools_health__", {}, data)["id"] == "otools-releasehub"
    snapshot = invoke("dispatch", {"action": "snapshot", "payload": {}}, data)
    assert snapshot["runtime"]["plugin"] is True and "buildTasks" not in snapshot
    imported = invoke("dispatch", {"action": "import_library", "payload": {"contents": json.dumps({"version": 1, "projects": [{"repository": "octocat/Hello-World", "channel": "stable", "preference": "auto", "assetPattern": None}], "settings": snapshot["settings"]})}}, data)
    assert imported["imported"] == 1
    saved = invoke("dispatch", {"action": "snapshot", "payload": {}}, data)
    assert saved["projects"][0]["repo"] == "Hello-World"
    assert invoke("poll_events", {}, data)["events"]
    invoke("__otools_shutdown__", {}, data)
print("Built native ABI smoke passed: health, isolated SQLite import, snapshot, events, shutdown")
