cmake_minimum_required(VERSION 3.21)
project(serika-code VERSION 0.2.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)

find_package(Qt6 REQUIRED COMPONENTS Quick QuickControls2 Network)

qt_standard_project_setup(REQUIRES 6.5)

file(GLOB SERIKA_QML RELATIVE ${CMAKE_CURRENT_SOURCE_DIR} CONFIGURE_DEPENDS qml/*.qml)
file(GLOB SERIKA_ICONS RELATIVE ${CMAKE_CURRENT_SOURCE_DIR} CONFIGURE_DEPENDS icons/*.svg)

# Keep every QML file next to the module's qmldir so Theme and Format resolve as singletons.
foreach(file ${SERIKA_QML})
    get_filename_component(name ${file} NAME)
    set_source_files_properties(${file} PROPERTIES QT_RESOURCE_ALIAS ${name})
endforeach()
set_source_files_properties(qml/Theme.qml qml/Format.qml PROPERTIES QT_QML_SINGLETON_TYPE TRUE)

qt_add_executable(serika-code
    src/main.cpp
    src/ipcclient.cpp
    src/ipcclient.h
    src/turnmodel.cpp
    src/turnmodel.h
)

qt_add_qml_module(serika-code
    URI SerikaCode
    VERSION 1.0
    QML_FILES ${SERIKA_QML}
    RESOURCES
        ${SERIKA_ICONS}
        fonts/Inter.ttf
        fonts/Outfit.ttf
        app/serika-code-16.png
        app/serika-code-32.png
        app/serika-code-48.png
        app/serika-code-64.png
        app/serika-code-128.png
        app/serika-code-256.png
)

# Explorer, the taskbar and shortcuts take the icon from the executable. Made by scripts/make_icon.py.
if(WIN32)
    enable_language(RC)
    target_sources(serika-code PRIVATE app/serika-code.rc)
endif()

target_link_libraries(serika-code PRIVATE Qt6::Quick Qt6::QuickControls2 Qt6::Network)

if(WIN32)
    set_target_properties(serika-code PROPERTIES WIN32_EXECUTABLE TRUE)
endif()
