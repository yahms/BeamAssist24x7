# Used by build.cmd: build whisper.cpp/ggml with the static C runtime (/MT) to match Rust's +crt-static.
# whisper.cpp declares cmake_minimum_required(3.5), so policy CMP0091 is unset and CMake takes the
# runtime from the default release flags ("/MD /O2 ..."), overriding cargo's -MT. Fix both ways:
set(CMAKE_POLICY_DEFAULT_CMP0091 NEW)
set(CMAKE_MSVC_RUNTIME_LIBRARY "MultiThreaded" CACHE STRING "MSVC runtime" FORCE)
foreach(lang C CXX)
  set(CMAKE_${lang}_FLAGS_RELEASE "/MT /O2 /Ob2 /DNDEBUG" CACHE STRING "" FORCE)
  set(CMAKE_${lang}_FLAGS_RELWITHDEBINFO "/MT /Zi /O2 /Ob1 /DNDEBUG" CACHE STRING "" FORCE)
  set(CMAKE_${lang}_FLAGS_MINSIZEREL "/MT /O1 /Ob1 /DNDEBUG" CACHE STRING "" FORCE)
endforeach()
