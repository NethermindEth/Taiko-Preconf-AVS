cmake_minimum_required(VERSION 3.10)
project(SignerLib)

# Build Go shared library
add_custom_command(
    OUTPUT libsigner.so
    COMMAND go build -o libsigner.so -buildmode=c-shared main.go
    WORKING_DIRECTORY ${CMAKE_SOURCE_DIR}
    DEPENDS main.go
)

add_custom_target(
    signer ALL
    DEPENDS libsigner.so
)
