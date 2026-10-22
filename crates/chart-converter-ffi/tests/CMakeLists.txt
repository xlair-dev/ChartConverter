cmake_minimum_required(VERSION 3.20)
project(ChartConverterFFISmoke LANGUAGES C CXX)

find_package(ChartConverterFFI CONFIG REQUIRED)
set(CMAKE_MSVC_RUNTIME_LIBRARY "MultiThreaded$<$<CONFIG:Debug>:Debug>")
enable_testing()

add_executable(c_api_smoke smoke.c)
set_target_properties(c_api_smoke PROPERTIES C_STANDARD 11 C_STANDARD_REQUIRED YES)
target_link_libraries(c_api_smoke PRIVATE ChartConverter::FFI)
add_test(NAME c_api_smoke COMMAND c_api_smoke)

add_executable(cpp_api_smoke smoke.cpp)
target_compile_features(cpp_api_smoke PRIVATE cxx_std_17)
target_link_libraries(cpp_api_smoke PRIVATE ChartConverter::FFI)
add_test(NAME cpp_api_smoke COMMAND cpp_api_smoke)
