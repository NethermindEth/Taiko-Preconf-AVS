package main

/*
#include <stdint.h>
#include <stdlib.h>
*/
import "C"

import (
	"github.com/decred/dcrd/dcrec/secp256k1/v4"
	"libsigner/signerk"
	"unsafe"
)

//export GetSignature
func GetSignature(inputHash *C.uint8_t) *C.uint8_t {
	// Create a signer
	signer, err := signerk.NewFixedKSigner("0x92954368afd3caa1f3ce3ead0069c1af414054aefe1ef9aeacc1bf426222ce38")
	if err != nil {
		return nil // failure
	}

	// Sign
	hash := C.GoBytes(unsafe.Pointer(inputHash), 32)
	sig, ok := signer.SignWithK(new(secp256k1.ModNScalar).SetInt(1))(hash)
	if !ok {
		sig, ok = signer.SignWithK(new(secp256k1.ModNScalar).SetInt(2))(hash)
		if !ok {
			return nil // failure
		}
	}

	if len(sig) != 65 {
		return nil // failure
	}

	// Allocate C memory for the returned array and copy the signature into it
	cArray := C.malloc(C.size_t(65))

	// Convert the Go byte slice (sig) into a C uint8_t slice and copy it to the allocated memory
	cSig := (*[65]C.uint8_t)(unsafe.Pointer(cArray))
	for i := 0; i < 65; i++ {
		cSig[i] = C.uint8_t(sig[i])
	}

	// Return the pointer to the C array
	return (*C.uint8_t)(cArray)
}

//export FreeBytesArray
func FreeBytesArray(ptr *C.uint8_t) {
	// Gracefully free the memory after use
    C.free(unsafe.Pointer(ptr))
}

func main() {} // Required for Go shared libraries
