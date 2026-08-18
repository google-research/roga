cmake_minimum_required(VERSION 3.16)
project(h2o2ram_ffi
  DESCRIPTION "C ABI for the H2O2RAM oblivious map"
  LANGUAGES CXX)

# Compile time geometry. Changing any of these changes the block size, which
# means the tuning file the hash planner looks for changes too.
set(H2O2RAM_KEY_MAX 32 CACHE STRING "Maximum key length in bytes")
set(H2O2RAM_VAL_SIZE 32 CACHE STRING "Exact value length in bytes")
set(H2O2RAM_CAPACITY 65536 CACHE STRING "Max live entries per map, rounded up to 128 * 2^k")

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS ON)

# Same rationale as the top level build: header-only templates at -O3 make
# every rebuild expensive.
find_program(CCACHE_PROGRAM ccache)
if(CCACHE_PROGRAM)
  set(CMAKE_CXX_COMPILER_LAUNCHER "${CCACHE_PROGRAM}")
endif()

find_package(TBB REQUIRED)
find_package(OpenMP REQUIRED)
find_package(Threads REQUIRED)
find_package(NLopt REQUIRED)
find_package(OpenSSL REQUIRED)

set(H2O2RAM_ROOT ${CMAKE_CURRENT_SOURCE_DIR}/..)

# graphs.cpp is not in this list: it pulls in the SSSP code, which the map does
# not touch.
add_library(h2o2ram_ffi STATIC
  h2o2ram_ffi.cpp
  ${H2O2RAM_ROOT}/src/oblivious_operations.cpp
  ${H2O2RAM_ROOT}/src/depthCounter.cpp
  ${H2O2RAM_ROOT}/src/timer.cpp)

target_include_directories(h2o2ram_ffi PUBLIC
  ${H2O2RAM_ROOT}/include
  ${CMAKE_CURRENT_SOURCE_DIR})

target_compile_definitions(h2o2ram_ffi PRIVATE
  H2O2RAM_KEY_MAX=${H2O2RAM_KEY_MAX}
  H2O2RAM_VAL_SIZE=${H2O2RAM_VAL_SIZE}
  MIN_CAPACITY=${H2O2RAM_CAPACITY})

target_compile_options(h2o2ram_ffi PRIVATE
  -O3 -march=native -Wno-deprecated-declarations)

target_link_libraries(h2o2ram_ffi PUBLIC
  TBB::tbb
  Threads::Threads
  OpenMP::OpenMP_CXX
  OpenSSL::Crypto
  nlopt)

install(TARGETS h2o2ram_ffi ARCHIVE DESTINATION lib)
install(FILES h2o2ram_ffi.h DESTINATION include)
