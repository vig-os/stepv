# KF6 thumbnail plugin for Dolphin (S4, vig-os/stepv#7). Needs Qt6 and KF6 KIO;
# runs the `stepv` CLI, which must be on PATH.
cmake_minimum_required(VERSION 3.24)
project(stepvthumbnail LANGUAGES CXX)

find_package(ECM 6.0 REQUIRED NO_MODULE)
set(CMAKE_MODULE_PATH ${ECM_MODULE_PATH})
include(KDEInstallDirs)
include(KDECMakeSettings)
include(KDECompilerSettings NO_POLICY_SCOPE)

find_package(Qt6 REQUIRED COMPONENTS Core Gui)
find_package(KF6 REQUIRED COMPONENTS KIO)

kcoreaddons_add_plugin(stepvthumbnail SOURCES stepvthumbnail.cpp INSTALL_NAMESPACE "kf6/thumbcreator")
target_link_libraries(stepvthumbnail PRIVATE KF6::KIOGui Qt6::Gui)
