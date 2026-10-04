# stepv-occt — the Plan B native-OCCT kernel (plan.md §3).
# Built by `just kernel`; the harness and the CLI find it at
# target/kernel/stepv-occt (or $STEPV_OCCT).
cmake_minimum_required(VERSION 3.24)
project(stepv-occt LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
if(NOT CMAKE_BUILD_TYPE)
  set(CMAKE_BUILD_TYPE Release)
endif()

# 7.8 is the floor: it renamed the STEP/IGES toolkits to TKDESTEP/TKDEIGES.
# Checked by hand because OCCT's version file only accepts the same minor.
find_package(OpenCASCADE REQUIRED CONFIG)
if(OpenCASCADE_VERSION VERSION_LESS 7.8)
  message(FATAL_ERROR "stepv-occt needs OCCT >= 7.8, found ${OpenCASCADE_VERSION}")
endif()

add_executable(stepv-occt stepv-occt.cpp)
target_include_directories(stepv-occt PRIVATE ${OpenCASCADE_INCLUDE_DIR})
target_compile_options(stepv-occt PRIVATE -Wall -Wextra)
target_link_libraries(stepv-occt PRIVATE
  TKDESTEP TKDEIGES TKXCAF TKXSBase TKLCAF TKCAF TKCDF
  TKMesh TKShHealing TKTopAlgo TKBRep TKGeomAlgo TKGeomBase TKG3d TKG2d
  TKMath TKernel)

# Generator for the synthetic `stress-assembly` fixture (`just fixtures`).
add_executable(stress-gen stress-gen.cpp)
target_include_directories(stress-gen PRIVATE ${OpenCASCADE_INCLUDE_DIR})
target_link_libraries(stress-gen PRIVATE TKDESTEP TKXSBase TKTopAlgo TKBRep TKMath TKernel TKG3d)

# Generator for the committed test corpus in tests/data/ (`just test-data`).
add_executable(fixture-gen fixture-gen.cpp)
target_include_directories(fixture-gen PRIVATE ${OpenCASCADE_INCLUDE_DIR})
target_link_libraries(fixture-gen PRIVATE
  TKDESTEP TKDEIGES TKXCAF TKXSBase TKLCAF TKCAF TKCDF TKBO TKPrim
  TKTopAlgo TKBRep TKGeomBase TKG3d TKMath TKernel)

# The kernel ships; the generators are dev tools.
install(TARGETS stepv-occt RUNTIME DESTINATION libexec/stepv)
