# Toolchain file for C dependencies built with CMake (SDL), passed in by .cargo/config.toml.
# Rust links the C runtime statically (`+crt-static`); CMake picks the runtime through this
# variable, not compiler flags, so it has to be set here to match.
set(CMAKE_MSVC_RUNTIME_LIBRARY "MultiThreaded")
